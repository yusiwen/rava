//! The class-object natives.
//!
//! `Object.getClass` hands out the mirror; the `Class` methods answer from the VM's `class_targets` map, and `initClassName` caches its answer in the object's `name` field.

use rava_vm::native::expect_args;
use rava_vm::{Array, ClassTarget, HeapObject, NativeCtx, Shared, Value, VmError};
use rava_vm::{ClassId, ObjRef};

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
    // Access checks in the reflection stack ask for the class's access flags.
    shared.register_native(
        "jdk/internal/reflect/Reflection",
        "getClassAccessFlags",
        "(Ljava/lang/Class;)I",
        class_access_flags,
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
    // The class loader asks every class for enclosing-method info while defining it. The VM does
    // not parse the `EnclosingMethod` attribute yet, so the honest answer is "none": `null`
    // (the class loader treats it as absent, which is true for everything but local/anonymous
    // classes).
    shared.register_native(
        "java/lang/Class",
        "getEnclosingMethod0",
        "()[Ljava/lang/Object;",
        no_enclosing_method,
    );
    // No `InnerClasses` parsing yet: every class reports itself as top-level.
    shared.register_native(
        "java/lang/Class",
        "getDeclaringClass0",
        "()Ljava/lang/Class;",
        no_declaring_class,
    );
    shared.register_native(
        "java/lang/Class",
        "getSimpleBinaryName0",
        "()Ljava/lang/String;",
        no_simple_binary_name,
    );
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
    // Reflection constructors: the objects `Class.getDeclaredConstructors()` returns. The
    // `Constructor` class's own Java methods read `clazz`/`parameterTypes`/`modifiers` from its
    // fields, so the native builds real instances rather than a VM-private placeholder.
    // The JDK's field accessors are built by `MethodHandleAccessorFactory` (a MethodHandle
    // surface this VM does not have yet), so `Field.get`/`set` and the primitive variants are
    // intrinsics over the VM's field slots.
    register_field_intrinsics(shared);
    // `Method.invoke` is an accessor in the JDK too (MethodAccessor); the intrinsic keeps the
    // public entry point and does allocate/receiver/unbox/call.
    shared.register_intrinsic(
        "java/lang/reflect/Method",
        "invoke",
        "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
        method_invoke,
    );
    // Reflection fields and methods: same shape as the constructors (real objects whose
    // Java methods read their fields), with `slot` naming the declared member.
    shared.register_native(
        "java/lang/Class",
        "getDeclaredFields0",
        "(Z)[Ljava/lang/reflect/Field;",
        declared_fields,
    );
    shared.register_native(
        "java/lang/Class",
        "getDeclaredMethods0",
        "(Z)[Ljava/lang/reflect/Method;",
        declared_methods,
    );
    shared.register_native(
        "java/lang/Class",
        "getDeclaredConstructors0",
        "(Z)[Ljava/lang/reflect/Constructor;",
        declared_constructors,
    );
    // `Constructor.newInstance` normally goes through the JDK's generated accessor
    // (`ConstructorAccessor`, a MethodHandle-shaped path this VM does not have). The intrinsic
    // keeps the public entry point and does the two things a constructor call needs: allocate
    // and invoke `<init>`.
    shared.register_intrinsic(
        "java/lang/reflect/Constructor",
        "newInstance",
        "([Ljava/lang/Object;)Ljava/lang/Object;",
        constructor_new_instance,
    );
    shared.register_native(
        "java/lang/Class",
        "forName0",
        "(Ljava/lang/String;ZLjava/lang/ClassLoader;Ljava/lang/Class;)Ljava/lang/Class;",
        for_name,
    );
}

/// `Reflection.getClassAccessFlags(Class)`: the class file's access flags; a primitive or array
/// class reports `public final` (there is no class file behind it).
fn class_access_flags(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [class] = expect_args::<1>(ctx)?;
    let class = class.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *class,
    })?;
    let flags = match ctx.shared.class_targets.get(&class) {
        Some(rava_vm::ClassTarget::Loaded(class)) => ctx
            .shared
            .class(*class)
            .file
            .as_ref()
            .map(|file| file.access_flags)
            .unwrap_or(0x0011),
        // `public final` is what HotSpot reports for a primitive; arrays add `abstract`.
        Some(rava_vm::ClassTarget::Type(descriptor)) => {
            if descriptor.starts_with('[') {
                0x0411
            } else {
                0x0011
            }
        }
        None => 0,
    };
    Ok(Value::Int(i32::from(flags)))
}

/// `StringUTF16.isBigEndian()`: false on this little-endian host.
fn is_big_endian(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::Int(0))
}

/// Registers the `Field` accessor intrinsics.
fn register_field_intrinsics(shared: &mut Shared) {
    shared.register_intrinsic(
        "java/lang/reflect/Field",
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        field_get,
    );
    shared.register_intrinsic(
        "java/lang/reflect/Field",
        "set",
        "(Ljava/lang/Object;Ljava/lang/Object;)V",
        field_set,
    );
    macro_rules! field_pair {
        ($get:ident, $set:ident, $java_get:expr, $java_set:expr, $get_desc:expr, $set_desc:expr) => {
            shared.register_intrinsic("java/lang/reflect/Field", $java_get, $get_desc, $get);
            shared.register_intrinsic("java/lang/reflect/Field", $java_set, $set_desc, $set);
        };
    }
    field_pair!(
        field_get_int,
        field_set_int,
        "getInt",
        "setInt",
        "(Ljava/lang/Object;)I",
        "(Ljava/lang/Object;I)V"
    );
    field_pair!(
        field_get_long,
        field_set_long,
        "getLong",
        "setLong",
        "(Ljava/lang/Object;)J",
        "(Ljava/lang/Object;J)V"
    );
    field_pair!(
        field_get_boolean,
        field_set_boolean,
        "getBoolean",
        "setBoolean",
        "(Ljava/lang/Object;)Z",
        "(Ljava/lang/Object;Z)V"
    );
    field_pair!(
        field_get_float,
        field_set_float,
        "getFloat",
        "setFloat",
        "(Ljava/lang/Object;)F",
        "(Ljava/lang/Object;F)V"
    );
    field_pair!(
        field_get_double,
        field_set_double,
        "getDouble",
        "setDouble",
        "(Ljava/lang/Object;)D",
        "(Ljava/lang/Object;D)V"
    );
}

/// The `(class, declared-field index)` a `Field` names.
fn field_member(ctx: &NativeCtx<'_>, field: ObjRef) -> Result<(ClassId, usize), VmError> {
    let class = ctx
        .shared
        .class_of(field)
        .ok_or_else(|| VmError::Native("not a Field object".to_owned()))?;
    let clazz_slot = ctx
        .shared
        .class(class)
        .field_slot("clazz", "Ljava/lang/Class;")
        .ok_or_else(|| VmError::Native("Field has no clazz field".to_owned()))?;
    let declaring = ctx
        .shared
        .instance_field(field, clazz_slot)?
        .as_ref()
        .ok_or(VmError::Native("Field.clazz is null".to_owned()))?;
    let target = match ctx.shared.class_targets.get(&declaring) {
        Some(rava_vm::ClassTarget::Loaded(target)) => *target,
        _ => return Err(VmError::Native("Field needs a loaded class".to_owned())),
    };
    let slot_slot = ctx
        .shared
        .class(class)
        .field_slot("slot", "I")
        .ok_or_else(|| VmError::Native("Field has no slot field".to_owned()))?;
    let index = ctx
        .shared
        .instance_field(field, slot_slot)?
        .as_int()
        .ok_or_else(|| VmError::Native("Field.slot is not an int".to_owned()))?
        as usize;
    Ok((target, index))
}

fn read_field_raw(
    ctx: &mut NativeCtx<'_>,
    field: ObjRef,
    receiver: Value,
) -> Result<(String, Value), VmError> {
    let (class, index) = field_member(ctx, field)?;
    let (name, descriptor, is_static) = {
        let info =
            ctx.shared.class(class).fields.get(index).ok_or_else(|| {
                VmError::Native(format!("field index {index} is outside the class"))
            })?;
        (
            info.name.clone(),
            info.descriptor.clone(),
            info.access_flags & rava_vm::class::ACC_STATIC != 0,
        )
    };
    if is_static {
        let value =
            *ctx.shared
                .class(class)
                .statics
                .get(&name)
                .ok_or_else(|| VmError::FieldNotFound {
                    class: ctx.shared.class(class).name.clone(),
                    name: name.clone(),
                })?;
        Ok((descriptor, value))
    } else {
        let receiver = receiver.as_ref().ok_or(VmError::TypeMismatch {
            expected: "an instance for an instance field",
            found: receiver,
        })?;
        let slot = ctx
            .shared
            .class(class)
            .field_slot(&name, &descriptor)
            .ok_or(VmError::FieldNotFound {
                class: ctx.shared.class(class).name.clone(),
                name: name.clone(),
            })?;
        Ok((descriptor, ctx.shared.instance_field(receiver, slot)?))
    }
}

fn write_field_raw(
    ctx: &mut NativeCtx<'_>,
    field: ObjRef,
    receiver: Value,
    value: Value,
) -> Result<(), VmError> {
    let (class, index) = field_member(ctx, field)?;
    let (name, descriptor, is_static) = {
        let info =
            ctx.shared.class(class).fields.get(index).ok_or_else(|| {
                VmError::Native(format!("field index {index} is outside the class"))
            })?;
        (
            info.name.clone(),
            info.descriptor.clone(),
            info.access_flags & rava_vm::class::ACC_STATIC != 0,
        )
    };
    if is_static {
        ctx.shared.classes[class.index()]
            .statics
            .insert(name, value);
        Ok(())
    } else {
        let receiver = receiver.as_ref().ok_or(VmError::TypeMismatch {
            expected: "an instance for an instance field",
            found: receiver,
        })?;
        let slot = ctx
            .shared
            .class(class)
            .field_slot(&name, &descriptor)
            .ok_or(VmError::FieldNotFound {
                class: ctx.shared.class(class).name.clone(),
                name: name.clone(),
            })?;
        ctx.shared.set_instance_field(receiver, slot, value)
    }
}

fn int_field_value(value: Value) -> Result<Value, VmError> {
    value.as_int().map(Value::Int).ok_or(VmError::TypeMismatch {
        expected: "an int field",
        found: value,
    })
}
fn long_field_value(value: Value) -> Result<Value, VmError> {
    value
        .as_long()
        .map(Value::Long)
        .ok_or(VmError::TypeMismatch {
            expected: "a long field",
            found: value,
        })
}
fn boolean_field_value(value: Value) -> Result<Value, VmError> {
    value.as_int().map(Value::Int).ok_or(VmError::TypeMismatch {
        expected: "a boolean field",
        found: value,
    })
}
fn float_field_value(value: Value) -> Result<Value, VmError> {
    value
        .as_float()
        .map(Value::Float)
        .ok_or(VmError::TypeMismatch {
            expected: "a float field",
            found: value,
        })
}
fn double_field_value(value: Value) -> Result<Value, VmError> {
    value
        .as_double()
        .map(Value::Double)
        .ok_or(VmError::TypeMismatch {
            expected: "a double field",
            found: value,
        })
}

macro_rules! primitive_field_pair {
    ($get:ident, $set:ident, $coerce:ident) => {
        fn $get(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
            let args = expect_args::<2>(ctx)?;
            let (this, receiver) = (args[0], args[1]);
            let field = this.as_ref().ok_or(VmError::TypeMismatch {
                expected: "a Field",
                found: this,
            })?;
            let (_descriptor, value) = read_field_raw(ctx, field, receiver)?;
            $coerce(value)
        }

        fn $set(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
            let args = expect_args::<3>(ctx)?;
            let (this, receiver, value) = (args[0], args[1], args[2]);
            let field = this.as_ref().ok_or(VmError::TypeMismatch {
                expected: "a Field",
                found: this,
            })?;
            write_field_raw(ctx, field, receiver, value)?;
            Ok(Value::NULL)
        }
    };
}

primitive_field_pair!(field_get_int, field_set_int, int_field_value);
primitive_field_pair!(field_get_long, field_set_long, long_field_value);
primitive_field_pair!(field_get_boolean, field_set_boolean, boolean_field_value);
primitive_field_pair!(field_get_float, field_set_float, float_field_value);
primitive_field_pair!(field_get_double, field_set_double, double_field_value);

/// `Field.get(Object)`: the field value, boxed when the field is primitive.
fn field_get(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let args = expect_args::<2>(ctx)?;
    let (this, receiver) = (args[0], args[1]);
    let field = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a Field",
        found: this,
    })?;
    let (descriptor, value) = read_field_raw(ctx, field, receiver)?;
    box_value(ctx, &descriptor, value)
}

/// `Field.set(Object, Object)`: unbox a primitive argument and store it.
fn field_set(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let args = expect_args::<3>(ctx)?;
    let (this, receiver, value) = (args[0], args[1], args[2]);
    let field = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a Field",
        found: this,
    })?;
    let (class, index) = field_member(ctx, field)?;
    let descriptor = ctx.shared.class(class).fields[index].descriptor.clone();
    let value = unbox(ctx, &descriptor, value)?;
    write_field_raw(ctx, field, receiver, value)?;
    Ok(Value::NULL)
}

/// Boxes a primitive value for `Field.get` (references pass through).
fn box_value(ctx: &mut NativeCtx<'_>, descriptor: &str, value: Value) -> Result<Value, VmError> {
    let (class_name, method) = match descriptor {
        "I" => ("java/lang/Integer", "valueOf"),
        "J" => ("java/lang/Long", "valueOf"),
        "Z" => ("java/lang/Boolean", "valueOf"),
        "B" => ("java/lang/Byte", "valueOf"),
        "S" => ("java/lang/Short", "valueOf"),
        "C" => ("java/lang/Character", "valueOf"),
        "F" => ("java/lang/Float", "valueOf"),
        "D" => ("java/lang/Double", "valueOf"),
        _ => return Ok(value),
    };
    let wrapper = ctx.shared.load_class(ctx.path, class_name)?;
    let (owner, index) = ctx
        .shared
        .resolve_method(wrapper, method, &format!("({descriptor})L{class_name};"))
        .ok_or_else(|| VmError::MethodNotFound {
            class: class_name.to_owned(),
            name: method.to_owned(),
            descriptor: format!("({descriptor})L{class_name};"),
        })?;
    let invoker = ctx
        .shared
        .invoker
        .ok_or_else(|| VmError::Native("Field.get needs the interpreter's call seam".to_owned()))?;
    invoker(ctx.shared, ctx.thread, ctx.path, owner, index, vec![value])
}

/// `Method.invoke(Object, Object...)`: run the method the `Method` object names. The receiver is
/// the first argument unless the method is static; boxed primitives are unboxed per descriptor.
fn method_invoke(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, receiver, arguments] = expect_args::<3>(ctx)?;
    let method = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a Method",
        found: *this,
    })?;
    let class = ctx.shared.class_of(method).ok_or(VmError::Native(
        "Method.invoke on an object that is not a Method".to_owned(),
    ))?;
    let clazz_slot = ctx
        .shared
        .class(class)
        .field_slot("clazz", "Ljava/lang/Class;")
        .ok_or_else(|| VmError::Native("Method has no clazz field".to_owned()))?;
    let declaring = ctx
        .shared
        .instance_field(method, clazz_slot)?
        .as_ref()
        .ok_or(VmError::Native("Method.clazz is null".to_owned()))?;
    let target = match ctx.shared.class_targets.get(&declaring) {
        Some(rava_vm::ClassTarget::Loaded(target)) => *target,
        _ => {
            return Err(VmError::Native(
                "Method.invoke needs a loaded class".to_owned(),
            ));
        }
    };
    let slot_slot = ctx
        .shared
        .class(class)
        .field_slot("slot", "I")
        .ok_or_else(|| VmError::Native("Method has no slot field".to_owned()))?;
    let index = ctx
        .shared
        .instance_field(method, slot_slot)?
        .as_int()
        .ok_or_else(|| VmError::Native("Method.slot is not an int".to_owned()))?
        as usize;
    let descriptor = ctx.shared.class(target).methods[index].descriptor.clone();
    let is_static =
        ctx.shared.class(target).methods[index].access_flags & rava_vm::class::ACC_STATIC != 0;
    let parameters = rava_vm::resolve::parameter_descriptors(&descriptor);
    let values = match arguments.as_ref() {
        Some(array) if !array.is_null() => ctx.shared.array(array)?.elements.clone(),
        _ if parameters.is_empty() => Vec::new(),
        _ => {
            return Err(VmError::Native(
                "Method.invoke arguments are null".to_owned(),
            ));
        }
    };
    let mut call_args = Vec::with_capacity(values.len() + 1);
    if !is_static {
        call_args.push(*receiver);
    }
    for (parameter, value) in parameters.iter().zip(values.iter()) {
        call_args.push(unbox(ctx, parameter, *value)?);
    }
    let return_descriptor = descriptor
        .split_once(')')
        .map(|(_, returns)| returns.to_owned())
        .unwrap_or_else(|| "V".to_owned());
    let invoker = ctx.shared.invoker.ok_or_else(|| {
        VmError::Native("Method.invoke needs the interpreter's call seam".to_owned())
    })?;
    let result = invoker(ctx.shared, ctx.thread, ctx.path, target, index, call_args)?;
    if return_descriptor == "V" {
        return Ok(Value::NULL);
    }
    // `Method.invoke` returns Object: a primitive result must come back boxed.
    box_value(ctx, &return_descriptor, result)
}

/// `Class.getDeclaredFields0(boolean)`: real `Field` objects, in declaration order. `slot` is
/// the field's index in the class's declared-field list; `Unsafe.objectFieldOffset` translates
/// it to the layout slot and the static variants encode it.
fn declared_fields(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, _public_only] = expect_args::<2>(ctx)?;
    let class_object = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *this,
    })?;
    let target = match ctx.shared.class_targets.get(&class_object) {
        Some(rava_vm::ClassTarget::Loaded(class)) => *class,
        _ => {
            return Ok(empty_member_array(ctx, "[Ljava/lang/reflect/Field;"));
        }
    };
    let field_class = ctx.shared.load_class(ctx.path, "java/lang/reflect/Field")?;
    initialize_class(ctx, field_class)?;
    let declaring = ctx.shared.class_object(ctx.path, target)?;
    let count = ctx.shared.class(target).fields.len();
    let mut elements = Vec::with_capacity(count);
    for index in 0..count {
        let (name, descriptor, modifiers) = {
            let field = &ctx.shared.class(target).fields[index];
            (
                field.name.clone(),
                field.descriptor.clone(),
                field.access_flags,
            )
        };
        let field = ctx.shared.new_instance(field_class);
        ctx.shared.pin(field);
        let name = ctx.shared.intern(ctx.path, &name)?;
        let field_type = class_for_descriptor(ctx, &descriptor)?;
        let annotations = empty_byte_array(ctx);
        ctx.shared.pin(annotations);
        for (field_name, description, value) in [
            ("clazz", "Ljava/lang/Class;", Value::Ref(declaring)),
            ("slot", "I", Value::Int(index as i32)),
            ("name", "Ljava/lang/String;", Value::Ref(name)),
            ("type", "Ljava/lang/Class;", Value::Ref(field_type)),
            ("modifiers", "I", Value::Int(i32::from(modifiers))),
            ("trustedFinal", "Z", Value::Int(0)),
            ("signature", "Ljava/lang/String;", Value::NULL),
            ("annotations", "[B", Value::Ref(annotations)),
        ] {
            set_member_field(ctx, field_class, field, field_name, description, value)?;
        }
        elements.push(Value::Ref(field));
    }
    let array = member_array(ctx, elements, "[Ljava/lang/reflect/Field;");
    Ok(Value::Ref(array))
}

/// `Class.getDeclaredMethods0(boolean)`: real `Method` objects (constructors and class
/// initializers are not methods). `slot` is the method's index in the class.
fn declared_methods(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, _public_only] = expect_args::<2>(ctx)?;
    let class_object = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *this,
    })?;
    let target = match ctx.shared.class_targets.get(&class_object) {
        Some(rava_vm::ClassTarget::Loaded(class)) => *class,
        _ => {
            return Ok(empty_member_array(ctx, "[Ljava/lang/reflect/Method;"));
        }
    };
    let method_class = ctx
        .shared
        .load_class(ctx.path, "java/lang/reflect/Method")?;
    initialize_class(ctx, method_class)?;
    let declaring = ctx.shared.class_object(ctx.path, target)?;
    let indices: Vec<usize> = (0..ctx.shared.class(target).methods.len())
        .filter(|index| {
            let method = &ctx.shared.class(target).methods[*index];
            method.name != "<init>" && method.name != "<clinit>"
        })
        .collect();
    let mut elements = Vec::with_capacity(indices.len());
    for index in indices {
        let (name, descriptor, modifiers) = {
            let method = &ctx.shared.class(target).methods[index];
            (
                method.name.clone(),
                method.descriptor.clone(),
                method.access_flags,
            )
        };
        let method = ctx.shared.new_instance(method_class);
        ctx.shared.pin(method);
        let name = ctx.shared.intern(ctx.path, &name)?;
        let return_descriptor = descriptor
            .split_once(')')
            .map(|(_, returns)| returns)
            .unwrap_or("V")
            .to_owned();
        let return_type = class_for_descriptor(ctx, &return_descriptor)?;
        let parameter_types = class_array(ctx, &descriptor)?;
        if let Value::Ref(array) = parameter_types {
            ctx.shared.pin(array);
        }
        let empty_classes = empty_class_array(ctx);
        if let Value::Ref(array) = empty_classes {
            ctx.shared.pin(array);
        }
        let annotations = empty_byte_array(ctx);
        ctx.shared.pin(annotations);
        let parameter_annotations = empty_byte_array(ctx);
        ctx.shared.pin(parameter_annotations);
        let annotation_default = empty_byte_array(ctx);
        ctx.shared.pin(annotation_default);
        for (field_name, description, value) in [
            ("clazz", "Ljava/lang/Class;", Value::Ref(declaring)),
            ("slot", "I", Value::Int(index as i32)),
            ("name", "Ljava/lang/String;", Value::Ref(name)),
            ("returnType", "Ljava/lang/Class;", Value::Ref(return_type)),
            ("parameterTypes", "[Ljava/lang/Class;", parameter_types),
            ("exceptionTypes", "[Ljava/lang/Class;", empty_classes),
            ("modifiers", "I", Value::Int(i32::from(modifiers))),
            ("signature", "Ljava/lang/String;", Value::NULL),
            ("annotations", "[B", Value::Ref(annotations)),
            (
                "parameterAnnotations",
                "[B",
                Value::Ref(parameter_annotations),
            ),
            ("annotationDefault", "[B", Value::Ref(annotation_default)),
        ] {
            set_member_field(ctx, method_class, method, field_name, description, value)?;
        }
        elements.push(Value::Ref(method));
    }
    let array = member_array(ctx, elements, "[Ljava/lang/reflect/Method;");
    Ok(Value::Ref(array))
}

fn set_member_field(
    ctx: &mut NativeCtx<'_>,
    class: ClassId,
    object: ObjRef,
    name: &str,
    descriptor: &str,
    value: Value,
) -> Result<(), VmError> {
    if let Some(slot) = ctx.shared.class(class).field_slot(name, descriptor) {
        ctx.shared.set_instance_field(object, slot, value)?;
    }
    Ok(())
}

fn empty_member_array(ctx: &mut NativeCtx<'_>, class_name: &str) -> Value {
    Value::Ref(member_array(ctx, Vec::new(), class_name))
}

fn member_array(ctx: &mut NativeCtx<'_>, elements: Vec<Value>, class_name: &str) -> ObjRef {
    let array = ctx
        .shared
        .allocate(rava_vm::HeapObject::Array(rava_vm::Array {
            kind: rava_vm::ArrayKind::Reference,
            elements,
            class_name: Some(class_name.to_owned()),
        }));
    ctx.shared.unpin_all();
    array
}

fn empty_byte_array(ctx: &mut NativeCtx<'_>) -> ObjRef {
    ctx.shared
        .allocate(rava_vm::HeapObject::Array(rava_vm::Array {
            kind: rava_vm::ArrayKind::Byte,
            elements: Vec::new(),
            class_name: Some("[B".to_owned()),
        }))
}

/// `Class.getDeclaredConstructors0(boolean)`: real `Constructor` objects for the class's
/// declared `<init>` methods, in declaration order. `publicOnly` is applied by the Java side
/// (`privateGetDeclaredConstructors`), which also caches the array.
fn declared_constructors(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, _public_only] = expect_args::<2>(ctx)?;
    let class_object = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *this,
    })?;
    let target = match ctx.shared.class_targets.get(&class_object) {
        Some(rava_vm::ClassTarget::Loaded(class)) => *class,
        _ => {
            let array = ctx
                .shared
                .allocate(rava_vm::HeapObject::Array(rava_vm::Array {
                    kind: rava_vm::ArrayKind::Reference,
                    elements: Vec::new(),
                    class_name: Some("[Ljava/lang/reflect/Constructor;".to_owned()),
                }));
            return Ok(Value::Ref(array));
        }
    };
    let constructor_class = ctx
        .shared
        .load_class(ctx.path, "java/lang/reflect/Constructor")?;
    // `AccessibleObject.<clinit>` registers the JDK's `JavaLangReflectAccess`; the JDK gets
    // there during boot, this VM reaches it through the superclass chain. Without it,
    // `ReflectionFactory.copyConstructor` NPEs on a null accessor.
    initialize_class(ctx, constructor_class)?;
    let indices: Vec<usize> = (0..ctx.shared.class(target).methods.len())
        .filter(|index| ctx.shared.class(target).methods[*index].name == "<init>")
        .collect();
    let mut elements = Vec::with_capacity(indices.len());
    for index in indices {
        let descriptor = ctx.shared.class(target).methods[index].descriptor.clone();
        let constructor = ctx.shared.new_instance(constructor_class);
        // The graph is assembled outside the heap's reach (no frame or static holds it yet):
        // pin each piece so a collection during the next allocation cannot free it.
        ctx.shared.pin(constructor);
        let parameter_types = class_array(ctx, &descriptor)?;
        if let Value::Ref(array) = parameter_types {
            ctx.shared.pin(array);
        }
        let class_object = ctx.shared.class_object(ctx.path, target)?;
        let modifiers = i32::from(ctx.shared.class(target).methods[index].access_flags);
        let empty_classes = empty_class_array(ctx);
        if let Value::Ref(array) = empty_classes {
            ctx.shared.pin(array);
        }
        let empty_bytes = ctx
            .shared
            .allocate(rava_vm::HeapObject::Array(rava_vm::Array {
                kind: rava_vm::ArrayKind::Byte,
                elements: Vec::new(),
                class_name: Some("[B".to_owned()),
            }));
        ctx.shared.pin(empty_bytes);
        for (name, description, value) in [
            ("clazz", "Ljava/lang/Class;", Value::Ref(class_object)),
            ("slot", "I", Value::Int(index as i32)),
            ("parameterTypes", "[Ljava/lang/Class;", parameter_types),
            ("exceptionTypes", "[Ljava/lang/Class;", empty_classes),
            ("modifiers", "I", Value::Int(modifiers)),
            ("signature", "Ljava/lang/String;", Value::NULL),
            ("annotations", "[B", Value::Ref(empty_bytes)),
            ("parameterAnnotations", "[B", Value::Ref(empty_bytes)),
        ] {
            let found = ctx
                .shared
                .class(constructor_class)
                .field_slot(name, description);
            if let Some(slot) = found {
                ctx.shared.set_instance_field(constructor, slot, value)?;
            }
        }
        elements.push(Value::Ref(constructor));
    }
    let array = ctx
        .shared
        .allocate(rava_vm::HeapObject::Array(rava_vm::Array {
            kind: rava_vm::ArrayKind::Reference,
            elements,
            class_name: Some("[Ljava/lang/reflect/Constructor;".to_owned()),
        }));
    // The array (returned on the operand stack) now roots everything it holds.
    ctx.shared.unpin_all();
    Ok(Value::Ref(array))
}

/// A `Class[]` of the parameter types of `descriptor`, in order.
fn class_array(ctx: &mut NativeCtx<'_>, descriptor: &str) -> Result<Value, VmError> {
    let parameters = rava_vm::resolve::parameter_descriptors(descriptor);
    let mut elements = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        elements.push(Value::Ref(class_for_descriptor(ctx, &parameter)?));
    }
    let array = ctx
        .shared
        .allocate(rava_vm::HeapObject::Array(rava_vm::Array {
            kind: rava_vm::ArrayKind::Reference,
            elements,
            class_name: Some("[Ljava/lang/Class;".to_owned()),
        }));
    Ok(Value::Ref(array))
}

fn empty_class_array(ctx: &mut NativeCtx<'_>) -> Value {
    let array = ctx
        .shared
        .allocate(rava_vm::HeapObject::Array(rava_vm::Array {
            kind: rava_vm::ArrayKind::Reference,
            elements: Vec::new(),
            class_name: Some("[Ljava/lang/Class;".to_owned()),
        }));
    Value::Ref(array)
}

fn class_for_descriptor(ctx: &mut NativeCtx<'_>, descriptor: &str) -> Result<ObjRef, VmError> {
    match descriptor {
        "I" | "J" | "F" | "D" | "Z" | "B" | "C" | "S" | "V" => {
            ctx.shared.type_class(ctx.path, descriptor)
        }
        array if array.starts_with('[') => ctx.shared.type_class(ctx.path, array),
        reference => {
            let name = reference
                .strip_prefix('L')
                .and_then(|name| name.strip_suffix(';'))
                .unwrap_or(reference);
            let class = ctx.shared.load_class(ctx.path, name)?;
            ctx.shared.class_object(ctx.path, class)
        }
    }
}

/// `Constructor.newInstance(Object...)`: allocate the declaring class and run `<init>` with the
/// (unboxed) arguments. Access checks are the JDK's job and are not modelled; the constructors
/// the library reflects on are public.
fn constructor_new_instance(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, arguments] = expect_args::<2>(ctx)?;
    let constructor = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a Constructor",
        found: *this,
    })?;
    let class = ctx.shared.class_of(constructor).ok_or(VmError::Native(
        "Constructor.newInstance on an object that is not a Constructor".to_owned(),
    ))?;
    let clazz_slot = ctx
        .shared
        .class(class)
        .field_slot("clazz", "Ljava/lang/Class;")
        .ok_or_else(|| VmError::Native("Constructor has no clazz field".to_owned()))?;
    let declaring = ctx
        .shared
        .instance_field(constructor, clazz_slot)?
        .as_ref()
        .ok_or(VmError::Native("Constructor.clazz is null".to_owned()))?;
    let target = match ctx.shared.class_targets.get(&declaring) {
        Some(rava_vm::ClassTarget::Loaded(target)) => *target,
        _ => {
            return Err(VmError::Native(
                "Constructor.newInstance needs a loaded class".to_owned(),
            ));
        }
    };
    let slot_slot = ctx
        .shared
        .class(class)
        .field_slot("slot", "I")
        .ok_or_else(|| VmError::Native("Constructor has no slot field".to_owned()))?;
    let index = ctx
        .shared
        .instance_field(constructor, slot_slot)?
        .as_int()
        .ok_or_else(|| VmError::Native("Constructor.slot is not an int".to_owned()))?
        as usize;
    let descriptor = ctx.shared.class(target).methods[index].descriptor.clone();
    let parameters = rava_vm::resolve::parameter_descriptors(&descriptor);
    let values = match arguments.as_ref() {
        Some(array) if !array.is_null() => ctx.shared.array(array)?.elements.clone(),
        _ if parameters.is_empty() => Vec::new(),
        _ => {
            return Err(VmError::Native(
                "Constructor.newInstance arguments are null".to_owned(),
            ));
        }
    };
    let receiver = ctx.shared.new_instance(target);
    let mut call_args = Vec::with_capacity(values.len() + 1);
    call_args.push(Value::Ref(receiver));
    for (parameter, value) in parameters.iter().zip(values.iter()) {
        call_args.push(unbox(ctx, parameter, *value)?);
    }
    let invoker = ctx.shared.invoker.ok_or_else(|| {
        VmError::Native("Constructor.newInstance needs the interpreter's call seam".to_owned())
    })?;
    invoker(ctx.shared, ctx.thread, ctx.path, target, index, call_args)?;
    Ok(Value::Ref(receiver))
}

/// Converts a boxed argument to the constructor's parameter type. References pass through.
fn unbox(ctx: &mut NativeCtx<'_>, parameter: &str, value: Value) -> Result<Value, VmError> {
    let primitive = matches!(parameter, "Z" | "B" | "C" | "S" | "I" | "J" | "F" | "D");
    if !primitive {
        return Ok(value);
    }
    let reference = value.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a boxed primitive",
        found: value,
    })?;
    let class = ctx.shared.class_of(reference).ok_or(VmError::Native(
        "boxed argument has no runtime class".to_owned(),
    ))?;
    let slot = ctx
        .shared
        .class(class)
        .field_slot("value", parameter)
        .or_else(|| {
            // `Character.value` is a char, `Boolean.value` a boolean: match by name and take
            // whatever int-typed value the field holds.
            ctx.shared
                .class(class)
                .fields
                .iter()
                .position(|field| field.name == "value")
        })
        .ok_or_else(|| {
            VmError::Native(format!(
                "cannot unbox a {} for a {parameter} parameter",
                ctx.shared.class(class).name
            ))
        })?;
    let stored = ctx.shared.instance_field(reference, slot)?;
    Ok(match parameter {
        "J" => Value::Long(stored.as_long().unwrap_or(0)),
        "F" => Value::Float(stored.as_float().unwrap_or(0.0)),
        "D" => Value::Double(stored.as_double().unwrap_or(0.0)),
        _ => Value::Int(stored.as_int().unwrap_or(0)),
    })
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
pub(crate) fn initialize_class(ctx: &mut NativeCtx<'_>, class: ClassId) -> Result<(), VmError> {
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

/// `Class.getEnclosingMethod0()`: absent until the VM parses the `EnclosingMethod` attribute.
fn no_enclosing_method(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::NULL)
}

/// `Class.getDeclaringClass0()`: `null` until `InnerClasses` parsing exists.
fn no_declaring_class(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::NULL)
}

/// `Class.getSimpleBinaryName0()`: `null` until `InnerClasses` parsing exists.
fn no_simple_binary_name(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::NULL)
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
    match ctx.shared.heap.get(reference.index()) {
        Some(HeapObject::Array(array)) => {
            let copy = Array {
                kind: array.kind,
                elements: array.elements.clone(),
                class_name: array.class_name.clone(),
            };
            Ok(Value::Ref(ctx.shared.allocate(HeapObject::Array(copy))))
        }
        Some(HeapObject::Instance(instance)) => {
            // `Object.clone` is a shallow copy: same class, same field values.
            let class = instance.class;
            let fields = instance.fields.clone();
            let copy = ctx.shared.new_instance(class);
            for (slot, value) in fields.into_iter().enumerate() {
                ctx.shared.set_instance_field(copy, slot, value)?;
            }
            Ok(Value::Ref(copy))
        }
        Some(HeapObject::Null) => Err(VmError::TypeMismatch {
            expected: "a reference",
            found: Value::NULL,
        }),
        Some(other) => Err(VmError::Unsupported {
            feature: "Object.clone",
            detail: format!("cannot clone {other:?}"),
        }),
        None => Err(VmError::Native(format!(
            "clone on dangling reference {reference:?}"
        ))),
    }
}
