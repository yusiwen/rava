//! M2.8 acceptance: reflected `Field`, `Method` and `Constructor` objects.
//!
//! `ReflectMembersProbe` is assembled against the committed C1 bootstrap library. It
//! reads the declared members of a class through the real `Class` and member stubs,
//! walks the returned arrays with `arraylength`/`aaload`, and calls the read-only member
//! methods. The only non-reflection helper is `same(String, String)`, a test-only native
//! used to match a member name; it keeps the fixture independent of the string natives
//! M2.9 adds later, and the point here is the member object, not string comparison.
//!
//! Expected values are derived from the same `ClassFile` the VM reads, so the tests
//! assert exact metadata without hard-coding JDK-private member counts.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_instance_method, run_static_method};
use rava_vm::{
    ArrayKind, ClassId, LoaderId, MemberKind, ObjRef, Shared, ThreadState, Value, VmError,
    reflection,
};

const ACC_PUBLIC_STATIC: u16 = 0x0009;

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

/// A tiny `String` equality native, registered only by this test.
///
/// Names are compared here with a test-local native so this fixture does not depend on
/// the `String` natives; the fixture still performs the declared-member loop in bytecode.
fn same(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(left)) = args.first().copied() else {
        return Err(VmError::Native("same expects two Strings".to_owned()));
    };
    let Some(Value::Ref(right)) = args.get(1).copied() else {
        return Err(VmError::Native("same expects two Strings".to_owned()));
    };
    let equal = {
        let heap = shared.heap();
        let left = heap
            .string(left)
            .map_err(|error| VmError::Native(error.to_string()))?;
        let right = heap
            .string(right)
            .map_err(|error| VmError::Native(error.to_string()))?;
        left == right
    };
    Ok(Some(Value::Int(i32::from(equal))))
}

fn method(
    builder: &mut ClassBuilder,
    name: &str,
    descriptor: &str,
    max_stack: u16,
    max_locals: u16,
    body: Assembler,
) {
    builder.method(
        ACC_PUBLIC_STATIC,
        name,
        descriptor,
        Some((max_stack, max_locals, body.finish().unwrap())),
    );
}

/// The fixture class. Every method is a static helper the test invokes directly, so the
/// assertions run through the interpreter's decode/dispatch path and the native registry.
fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("ReflectMembersProbe", "java/lang/Object", 61);
    builder.field(0x0009, "counter", "I");
    builder.field(0x0009, "label", "Ljava/lang/String;");
    builder.field(0x0009, "other", "J");

    let object_get_class =
        builder.method_ref("java/lang/Object", "getClass", "()Ljava/lang/Class;");
    let class_get_name = builder.method_ref("java/lang/Class", "getName", "()Ljava/lang/String;");
    let class_for_name = builder.method_ref(
        "java/lang/Class",
        "forName",
        "(Ljava/lang/String;)Ljava/lang/Class;",
    );
    let get_declared_fields = builder.method_ref(
        "java/lang/Class",
        "getDeclaredFields",
        "()[Ljava/lang/reflect/Field;",
    );
    let get_declared_methods = builder.method_ref(
        "java/lang/Class",
        "getDeclaredMethods",
        "()[Ljava/lang/reflect/Method;",
    );
    let get_declared_constructors = builder.method_ref(
        "java/lang/Class",
        "getDeclaredConstructors",
        "()[Ljava/lang/reflect/Constructor;",
    );
    let field_get_name =
        builder.method_ref("java/lang/reflect/Field", "getName", "()Ljava/lang/String;");
    let field_get_type =
        builder.method_ref("java/lang/reflect/Field", "getType", "()Ljava/lang/Class;");
    let field_get_modifiers = builder.method_ref("java/lang/reflect/Field", "getModifiers", "()I");
    let field_get_declaring_class = builder.method_ref(
        "java/lang/reflect/Field",
        "getDeclaringClass",
        "()Ljava/lang/Class;",
    );
    let method_get_name = builder.method_ref(
        "java/lang/reflect/Method",
        "getName",
        "()Ljava/lang/String;",
    );
    let method_get_modifiers =
        builder.method_ref("java/lang/reflect/Method", "getModifiers", "()I");
    let method_get_declaring_class = builder.method_ref(
        "java/lang/reflect/Method",
        "getDeclaringClass",
        "()Ljava/lang/Class;",
    );
    let method_get_return_type = builder.method_ref(
        "java/lang/reflect/Method",
        "getReturnType",
        "()Ljava/lang/Class;",
    );
    let method_get_parameter_types = builder.method_ref(
        "java/lang/reflect/Method",
        "getParameterTypes",
        "()[Ljava/lang/Class;",
    );
    let method_get_parameter_count =
        builder.method_ref("java/lang/reflect/Method", "getParameterCount", "()I");
    let constructor_get_name = builder.method_ref(
        "java/lang/reflect/Constructor",
        "getName",
        "()Ljava/lang/String;",
    );
    let constructor_get_declaring_class = builder.method_ref(
        "java/lang/reflect/Constructor",
        "getDeclaringClass",
        "()Ljava/lang/Class;",
    );
    let constructor_get_parameter_count =
        builder.method_ref("java/lang/reflect/Constructor", "getParameterCount", "()I");
    let find_field = builder.method_ref(
        "ReflectMembersProbe",
        "findField",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/reflect/Field;",
    );
    let find_method = builder.method_ref(
        "ReflectMembersProbe",
        "findMethod",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/reflect/Method;",
    );
    let first_constructor = builder.method_ref(
        "ReflectMembersProbe",
        "firstConstructor",
        "(Ljava/lang/Class;)Ljava/lang/reflect/Constructor;",
    );
    let same = builder.method_ref(
        "ReflectMembersProbe",
        "same",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
    );
    let string_class_name = builder.string("java.lang.String");

    // static boolean same(String, String) is a native: the registry supplies the body.
    builder.method(
        ACC_PUBLIC_STATIC,
        "same",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        None,
    );

    // static int stringDeclaredFieldCount() {
    //     return Class.forName("java.lang.String").getDeclaredFields().length;
    // }
    let mut string_field_count = Assembler::new();
    string_field_count.ldc(string_class_name);
    string_field_count.op(0xb8, &class_for_name.to_be_bytes()); // invokestatic
    string_field_count.invokevirtual(get_declared_fields);
    string_field_count.arraylength();
    string_field_count.ireturn();
    method(
        &mut builder,
        "stringDeclaredFieldCount",
        "()I",
        1,
        0,
        string_field_count,
    );

    // static Field[] declaredFields(Class c) { return c.getDeclaredFields(); }
    let mut declared_fields = Assembler::new();
    declared_fields.aload(0);
    declared_fields.invokevirtual(get_declared_fields);
    declared_fields.areturn();
    method(
        &mut builder,
        "declaredFields",
        "(Ljava/lang/Class;)[Ljava/lang/reflect/Field;",
        1,
        1,
        declared_fields,
    );

    // static Method[] declaredMethods(Class c) { return c.getDeclaredMethods(); }
    let mut declared_methods = Assembler::new();
    declared_methods.aload(0);
    declared_methods.invokevirtual(get_declared_methods);
    declared_methods.areturn();
    method(
        &mut builder,
        "declaredMethods",
        "(Ljava/lang/Class;)[Ljava/lang/reflect/Method;",
        1,
        1,
        declared_methods,
    );

    // static Constructor[] declaredConstructors(Class c) { return c.getDeclaredConstructors(); }
    let mut declared_constructors = Assembler::new();
    declared_constructors.aload(0);
    declared_constructors.invokevirtual(get_declared_constructors);
    declared_constructors.areturn();
    method(
        &mut builder,
        "declaredConstructors",
        "(Ljava/lang/Class;)[Ljava/lang/reflect/Constructor;",
        1,
        1,
        declared_constructors,
    );

    // static Field findField(Class c, String name) {
    //     Field[] fields = c.getDeclaredFields();
    //     int i = 0;
    //     while (i < fields.length) {
    //         Field field = fields[i];
    //         if (same(field.getName(), name)) return field;
    //         i++;
    //     }
    //     return null;
    // }
    let mut find_field_body = Assembler::new();
    let field_loop = find_field_body.label();
    let field_next = find_field_body.label();
    let field_done = find_field_body.label();
    find_field_body.aload(0);
    find_field_body.invokevirtual(get_declared_fields);
    find_field_body.astore(2);
    find_field_body.iconst(0);
    find_field_body.istore(3);
    find_field_body.place(field_loop);
    find_field_body.iload(3);
    find_field_body.aload(2);
    find_field_body.arraylength();
    find_field_body.if_icmp(0xa2, field_done); // if_icmpge
    find_field_body.aload(2);
    find_field_body.iload(3);
    find_field_body.aaload();
    find_field_body.astore(4);
    find_field_body.aload(4);
    find_field_body.invokevirtual(field_get_name);
    find_field_body.aload(1);
    find_field_body.op(0xb8, &same.to_be_bytes()); // invokestatic
    find_field_body.if_cond(0x99, field_next); // ifeq
    find_field_body.aload(4);
    find_field_body.areturn();
    find_field_body.place(field_next);
    find_field_body.iinc(3, 1);
    find_field_body.goto(field_loop);
    find_field_body.place(field_done);
    find_field_body.aconst_null();
    find_field_body.areturn();
    method(
        &mut builder,
        "findField",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/reflect/Field;",
        3,
        5,
        find_field_body,
    );

    // static String fieldName(Class c, String name) { return findField(c, name).getName(); }
    let mut field_name = Assembler::new();
    field_name.aload(0);
    field_name.aload(1);
    field_name.op(0xb8, &find_field.to_be_bytes()); // invokestatic
    field_name.invokevirtual(field_get_name);
    field_name.areturn();
    method(
        &mut builder,
        "fieldName",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
        2,
        2,
        field_name,
    );

    // static String fieldTypeName(Class c, String name) {
    //     return findField(c, name).getType().getName();
    // }
    let mut field_type_name = Assembler::new();
    field_type_name.aload(0);
    field_type_name.aload(1);
    field_type_name.op(0xb8, &find_field.to_be_bytes()); // invokestatic
    field_type_name.invokevirtual(field_get_type);
    field_type_name.invokevirtual(class_get_name);
    field_type_name.areturn();
    method(
        &mut builder,
        "fieldTypeName",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
        2,
        2,
        field_type_name,
    );

    // static int fieldModifiers(Class c, String name) {
    //     return findField(c, name).getModifiers();
    // }
    let mut field_modifiers = Assembler::new();
    field_modifiers.aload(0);
    field_modifiers.aload(1);
    field_modifiers.op(0xb8, &find_field.to_be_bytes()); // invokestatic
    field_modifiers.invokevirtual(field_get_modifiers);
    field_modifiers.ireturn();
    method(
        &mut builder,
        "fieldModifiers",
        "(Ljava/lang/Class;Ljava/lang/String;)I",
        2,
        2,
        field_modifiers,
    );

    // static String fieldDeclaringClassName(Class c, String name) {
    //     return findField(c, name).getDeclaringClass().getName();
    // }
    let mut field_declaring = Assembler::new();
    field_declaring.aload(0);
    field_declaring.aload(1);
    field_declaring.op(0xb8, &find_field.to_be_bytes()); // invokestatic
    field_declaring.invokevirtual(field_get_declaring_class);
    field_declaring.invokevirtual(class_get_name);
    field_declaring.areturn();
    method(
        &mut builder,
        "fieldDeclaringClassName",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
        2,
        2,
        field_declaring,
    );

    // static String fieldRuntimeClassName(Class c, String name) {
    //     return findField(c, name).getClass().getName();
    // }
    let mut field_runtime = Assembler::new();
    field_runtime.aload(0);
    field_runtime.aload(1);
    field_runtime.op(0xb8, &find_field.to_be_bytes()); // invokestatic
    field_runtime.invokevirtual(object_get_class);
    field_runtime.invokevirtual(class_get_name);
    field_runtime.areturn();
    method(
        &mut builder,
        "fieldRuntimeClassName",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
        2,
        2,
        field_runtime,
    );

    // static Method findMethod(Class c, String name) {
    //     Method[] methods = c.getDeclaredMethods();
    //     int i = 0;
    //     while (i < methods.length) {
    //         Method method = methods[i];
    //         if (same(method.getName(), name)) return method;
    //         i++;
    //     }
    //     return null;
    // }
    let mut find_method_body = Assembler::new();
    let method_loop = find_method_body.label();
    let method_next = find_method_body.label();
    let method_done = find_method_body.label();
    find_method_body.aload(0);
    find_method_body.invokevirtual(get_declared_methods);
    find_method_body.astore(2);
    find_method_body.iconst(0);
    find_method_body.istore(3);
    find_method_body.place(method_loop);
    find_method_body.iload(3);
    find_method_body.aload(2);
    find_method_body.arraylength();
    find_method_body.if_icmp(0xa2, method_done); // if_icmpge
    find_method_body.aload(2);
    find_method_body.iload(3);
    find_method_body.aaload();
    find_method_body.astore(4);
    find_method_body.aload(4);
    find_method_body.invokevirtual(method_get_name);
    find_method_body.aload(1);
    find_method_body.op(0xb8, &same.to_be_bytes()); // invokestatic
    find_method_body.if_cond(0x99, method_next); // ifeq
    find_method_body.aload(4);
    find_method_body.areturn();
    find_method_body.place(method_next);
    find_method_body.iinc(3, 1);
    find_method_body.goto(method_loop);
    find_method_body.place(method_done);
    find_method_body.aconst_null();
    find_method_body.areturn();
    method(
        &mut builder,
        "findMethod",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/reflect/Method;",
        3,
        5,
        find_method_body,
    );

    // static String firstMethodName(Class c) {
    //     return c.getDeclaredMethods()[0].getName();
    // }
    let mut first_method_name = Assembler::new();
    first_method_name.aload(0);
    first_method_name.invokevirtual(get_declared_methods);
    first_method_name.iconst(0);
    first_method_name.aaload();
    first_method_name.invokevirtual(method_get_name);
    first_method_name.areturn();
    method(
        &mut builder,
        "firstMethodName",
        "(Ljava/lang/Class;)Ljava/lang/String;",
        2,
        1,
        first_method_name,
    );

    // static int methodCount(Class c) { return c.getDeclaredMethods().length; }
    let mut method_count = Assembler::new();
    method_count.aload(0);
    method_count.invokevirtual(get_declared_methods);
    method_count.arraylength();
    method_count.ireturn();
    method(
        &mut builder,
        "methodCount",
        "(Ljava/lang/Class;)I",
        1,
        1,
        method_count,
    );

    // static int methodParameterCount(Class c, String name) {
    //     return findMethod(c, name).getParameterCount();
    // }
    let mut method_parameter_count = Assembler::new();
    method_parameter_count.aload(0);
    method_parameter_count.aload(1);
    method_parameter_count.op(0xb8, &find_method.to_be_bytes()); // invokestatic
    method_parameter_count.invokevirtual(method_get_parameter_count);
    method_parameter_count.ireturn();
    method(
        &mut builder,
        "methodParameterCount",
        "(Ljava/lang/Class;Ljava/lang/String;)I",
        2,
        2,
        method_parameter_count,
    );

    // static String methodParameterTypeName(Class c, String name) {
    //     return findMethod(c, name).getParameterTypes()[0].getName();
    // }
    let mut method_parameter_type = Assembler::new();
    method_parameter_type.aload(0);
    method_parameter_type.aload(1);
    method_parameter_type.op(0xb8, &find_method.to_be_bytes()); // invokestatic
    method_parameter_type.invokevirtual(method_get_parameter_types);
    method_parameter_type.iconst(0);
    method_parameter_type.aaload();
    method_parameter_type.invokevirtual(class_get_name);
    method_parameter_type.areturn();
    method(
        &mut builder,
        "methodParameterTypeName",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
        2,
        2,
        method_parameter_type,
    );

    // static String methodReturnTypeName(Class c, String name) {
    //     return findMethod(c, name).getReturnType().getName();
    // }
    let mut method_return_type = Assembler::new();
    method_return_type.aload(0);
    method_return_type.aload(1);
    method_return_type.op(0xb8, &find_method.to_be_bytes()); // invokestatic
    method_return_type.invokevirtual(method_get_return_type);
    method_return_type.invokevirtual(class_get_name);
    method_return_type.areturn();
    method(
        &mut builder,
        "methodReturnTypeName",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
        2,
        2,
        method_return_type,
    );

    // static String methodDeclaringClassName(Class c, String name) {
    //     return findMethod(c, name).getDeclaringClass().getName();
    // }
    let mut method_declaring = Assembler::new();
    method_declaring.aload(0);
    method_declaring.aload(1);
    method_declaring.op(0xb8, &find_method.to_be_bytes()); // invokestatic
    method_declaring.invokevirtual(method_get_declaring_class);
    method_declaring.invokevirtual(class_get_name);
    method_declaring.areturn();
    method(
        &mut builder,
        "methodDeclaringClassName",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
        2,
        2,
        method_declaring,
    );

    // static int methodModifiers(Class c, String name) {
    //     return findMethod(c, name).getModifiers();
    // }
    let mut method_modifiers = Assembler::new();
    method_modifiers.aload(0);
    method_modifiers.aload(1);
    method_modifiers.op(0xb8, &find_method.to_be_bytes()); // invokestatic
    method_modifiers.invokevirtual(method_get_modifiers);
    method_modifiers.ireturn();
    method(
        &mut builder,
        "methodModifiers",
        "(Ljava/lang/Class;Ljava/lang/String;)I",
        2,
        2,
        method_modifiers,
    );

    // static String methodRuntimeClassName(Class c, String name) {
    //     return findMethod(c, name).getClass().getName();
    // }
    let mut method_runtime = Assembler::new();
    method_runtime.aload(0);
    method_runtime.aload(1);
    method_runtime.op(0xb8, &find_method.to_be_bytes()); // invokestatic
    method_runtime.invokevirtual(object_get_class);
    method_runtime.invokevirtual(class_get_name);
    method_runtime.areturn();
    method(
        &mut builder,
        "methodRuntimeClassName",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
        2,
        2,
        method_runtime,
    );

    // static Constructor firstConstructor(Class c) {
    //     return c.getDeclaredConstructors()[0];
    // }
    let mut first_constructor_body = Assembler::new();
    first_constructor_body.aload(0);
    first_constructor_body.invokevirtual(get_declared_constructors);
    first_constructor_body.iconst(0);
    first_constructor_body.aaload();
    first_constructor_body.areturn();
    method(
        &mut builder,
        "firstConstructor",
        "(Ljava/lang/Class;)Ljava/lang/reflect/Constructor;",
        2,
        1,
        first_constructor_body,
    );

    // static int constructorCount(Class c) { return c.getDeclaredConstructors().length; }
    let mut constructor_count = Assembler::new();
    constructor_count.aload(0);
    constructor_count.invokevirtual(get_declared_constructors);
    constructor_count.arraylength();
    constructor_count.ireturn();
    method(
        &mut builder,
        "constructorCount",
        "(Ljava/lang/Class;)I",
        1,
        1,
        constructor_count,
    );

    // static int firstConstructorParameterCount(Class c) {
    //     return firstConstructor(c).getParameterCount();
    // }
    let mut first_constructor_parameters = Assembler::new();
    first_constructor_parameters.aload(0);
    first_constructor_parameters.op(0xb8, &first_constructor.to_be_bytes()); // invokestatic
    first_constructor_parameters.invokevirtual(constructor_get_parameter_count);
    first_constructor_parameters.ireturn();
    method(
        &mut builder,
        "firstConstructorParameterCount",
        "(Ljava/lang/Class;)I",
        1,
        1,
        first_constructor_parameters,
    );

    // static String firstConstructorName(Class c) {
    //     return firstConstructor(c).getName();
    // }
    let mut first_constructor_name = Assembler::new();
    first_constructor_name.aload(0);
    first_constructor_name.op(0xb8, &first_constructor.to_be_bytes()); // invokestatic
    first_constructor_name.invokevirtual(constructor_get_name);
    first_constructor_name.areturn();
    method(
        &mut builder,
        "firstConstructorName",
        "(Ljava/lang/Class;)Ljava/lang/String;",
        1,
        1,
        first_constructor_name,
    );

    // static String firstConstructorDeclaringClassName(Class c) {
    //     return firstConstructor(c).getDeclaringClass().getName();
    // }
    let mut first_constructor_declaring = Assembler::new();
    first_constructor_declaring.aload(0);
    first_constructor_declaring.op(0xb8, &first_constructor.to_be_bytes()); // invokestatic
    first_constructor_declaring.invokevirtual(constructor_get_declaring_class);
    first_constructor_declaring.invokevirtual(class_get_name);
    first_constructor_declaring.areturn();
    method(
        &mut builder,
        "firstConstructorDeclaringClassName",
        "(Ljava/lang/Class;)Ljava/lang/String;",
        1,
        1,
        first_constructor_declaring,
    );

    // static String firstConstructorRuntimeClassName(Class c) {
    //     return firstConstructor(c).getClass().getName();
    // }
    let mut first_constructor_runtime = Assembler::new();
    first_constructor_runtime.aload(0);
    first_constructor_runtime.op(0xb8, &first_constructor.to_be_bytes()); // invokestatic
    first_constructor_runtime.invokevirtual(object_get_class);
    first_constructor_runtime.invokevirtual(class_get_name);
    first_constructor_runtime.areturn();
    method(
        &mut builder,
        "firstConstructorRuntimeClassName",
        "(Ljava/lang/Class;)Ljava/lang/String;",
        1,
        1,
        first_constructor_runtime,
    );

    builder.build()
}

struct Fixture {
    shared: Shared,
    probe: ClassId,
    thread: ThreadState,
    string_class: ClassId,
    string_object: ObjRef,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    reflection::register_natives(&shared);
    shared.register_native(
        "ReflectMembersProbe",
        "same",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        same,
    );
    let probe = shared
        .define_class(LoaderId::BOOTSTRAP, "ReflectMembersProbe", &probe_bytes())
        .expect("the fixture class defines");
    let string_class = shared
        .load_library_class("java/lang/String")
        .expect("java.lang.String loads");
    let string_object = shared
        .class_object(string_class)
        .expect("java.lang.String gets a class object");
    let thread = shared.thread_state(1);
    Fixture {
        shared,
        probe,
        thread,
        string_class,
        string_object,
    }
}

fn call(
    fixture: &Fixture,
    method: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        method,
        descriptor,
        args,
    )
}

fn string_value(shared: &Shared, value: Option<Value>) -> String {
    let Some(Value::Ref(reference)) = value else {
        panic!("expected a string result, found {value:?}");
    };
    shared
        .heap()
        .string(reference)
        .expect("the result is a string")
        .to_owned()
}

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int result, found {other:?}"),
    }
}

fn ref_value(value: Option<Value>) -> ObjRef {
    match value {
        Some(Value::Ref(reference)) => reference,
        other => panic!("expected a reference result, found {other:?}"),
    }
}

fn name_argument(fixture: &Fixture, name: &str) -> Value {
    Value::Ref(fixture.shared.heap().allocate_string(name))
}

fn string_class_argument(fixture: &Fixture) -> Value {
    Value::Ref(fixture.string_object)
}

/// Count the parameters of a method descriptor independently of the production parser.
fn parameter_count(descriptor: &str) -> usize {
    let parameters = descriptor
        .strip_prefix('(')
        .and_then(|rest| rest.split_once(')'))
        .map(|(parameters, _)| parameters)
        .expect("a method descriptor");
    let bytes = parameters.as_bytes();
    let mut cursor = 0;
    let mut count = 0;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'L' => {
                cursor += parameters[cursor..]
                    .find(';')
                    .expect("an object descriptor")
                    + 1;
            }
            b'[' => {
                while bytes.get(cursor) == Some(&b'[') {
                    cursor += 1;
                }
                match bytes.get(cursor).copied() {
                    Some(b'L') => {
                        cursor += parameters[cursor..]
                            .find(';')
                            .expect("an object descriptor")
                            + 1;
                    }
                    Some(_) => cursor += 1,
                    None => panic!("a truncated array descriptor"),
                }
            }
            _ => cursor += 1,
        }
        count += 1;
    }
    count
}

#[test]
fn declared_fields_are_cached_member_objects_in_a_reference_array() {
    let fixture = fixture();
    let array = ref_value(
        call(
            &fixture,
            "declaredFields",
            "(Ljava/lang/Class;)[Ljava/lang/reflect/Field;",
            vec![string_class_argument(&fixture)],
        )
        .unwrap(),
    );

    let elements = {
        let heap = fixture.shared.heap();
        let array = heap.array(array).expect("a real reference array");
        assert_eq!(array.kind, ArrayKind::Reference);
        array.elements.clone()
    };

    let entry = fixture.shared.entry(fixture.string_class).unwrap();
    assert_eq!(elements.len(), entry.file.fields.len());
    assert!(!elements.is_empty(), "String declares fields");

    let mut references = Vec::with_capacity(elements.len());
    for (index, value) in elements.iter().enumerate() {
        let Value::Ref(reference) = value else {
            panic!("a declared-field array holds references, found {value:?}");
        };
        let member = fixture.shared.heap().reflect(*reference).unwrap().clone();
        assert_eq!(member.kind, MemberKind::Field);
        assert_eq!(member.class, fixture.string_class);
        assert_eq!(
            member.name,
            entry.file.field_name(&entry.file.fields[index]).unwrap()
        );
        assert_eq!(
            member.descriptor,
            entry
                .file
                .field_descriptor(&entry.file.fields[index])
                .unwrap()
        );
        assert_eq!(
            fixture
                .shared
                .reflect_member(
                    fixture.string_class,
                    MemberKind::Field,
                    &member.name,
                    &member.descriptor,
                )
                .unwrap(),
            *reference,
            "one cached member object per (class, kind, name, descriptor)"
        );
        references.push(*reference);
    }

    // A second query returns the same member objects, not fresh copies.
    let again = ref_value(
        call(
            &fixture,
            "declaredFields",
            "(Ljava/lang/Class;)[Ljava/lang/reflect/Field;",
            vec![string_class_argument(&fixture)],
        )
        .unwrap(),
    );
    let again_elements = fixture.shared.heap().array(again).unwrap().elements.clone();
    assert_eq!(again_elements, elements);

    let roots = fixture.shared.roots(&[]);
    for reference in references {
        assert!(
            roots.contains(&reference),
            "cached member {reference} is a GC root"
        );
    }
}

#[test]
fn string_declares_case_insensitive_order_of_type_comparator() {
    let fixture = fixture();

    let count = int_value(call(&fixture, "stringDeclaredFieldCount", "()I", Vec::new()).unwrap());
    let expected = fixture
        .shared
        .entry(fixture.string_class)
        .unwrap()
        .file
        .fields
        .len();
    assert_eq!(usize::try_from(count).unwrap(), expected);
    assert!(expected > 0, "String declares fields");

    let found = string_value(
        &fixture.shared,
        call(
            &fixture,
            "fieldName",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
            vec![
                string_class_argument(&fixture),
                name_argument(&fixture, "CASE_INSENSITIVE_ORDER"),
            ],
        )
        .unwrap(),
    );
    assert_eq!(found, "CASE_INSENSITIVE_ORDER");

    let type_name = string_value(
        &fixture.shared,
        call(
            &fixture,
            "fieldTypeName",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
            vec![
                string_class_argument(&fixture),
                name_argument(&fixture, "CASE_INSENSITIVE_ORDER"),
            ],
        )
        .unwrap(),
    );
    assert_eq!(type_name, "java.util.Comparator");

    let modifiers = int_value(
        call(
            &fixture,
            "fieldModifiers",
            "(Ljava/lang/Class;Ljava/lang/String;)I",
            vec![
                string_class_argument(&fixture),
                name_argument(&fixture, "CASE_INSENSITIVE_ORDER"),
            ],
        )
        .unwrap(),
    );
    // `public static final`; the generic `Comparator<String>` signature is not reflected.
    assert_eq!(modifiers, 0x0019);

    let declaring = string_value(
        &fixture.shared,
        call(
            &fixture,
            "fieldDeclaringClassName",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
            vec![
                string_class_argument(&fixture),
                name_argument(&fixture, "CASE_INSENSITIVE_ORDER"),
            ],
        )
        .unwrap(),
    );
    assert_eq!(declaring, "java.lang.String");

    let runtime = string_value(
        &fixture.shared,
        call(
            &fixture,
            "fieldRuntimeClassName",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
            vec![
                string_class_argument(&fixture),
                name_argument(&fixture, "CASE_INSENSITIVE_ORDER"),
            ],
        )
        .unwrap(),
    );
    assert_eq!(runtime, "java.lang.reflect.Field");

    // `Field.getType()` is the cached `java.util.Comparator` class object, and the
    // member's own class object is `java.lang.reflect.Field`.
    let field = ref_value(
        call(
            &fixture,
            "findField",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/reflect/Field;",
            vec![
                string_class_argument(&fixture),
                name_argument(&fixture, "CASE_INSENSITIVE_ORDER"),
            ],
        )
        .unwrap(),
    );
    let comparator = fixture
        .shared
        .load_library_class("java/util/Comparator")
        .unwrap();
    let comparator_object = fixture.shared.class_object(comparator).unwrap();
    let field_class = fixture
        .shared
        .load_library_class("java/lang/reflect/Field")
        .unwrap();
    let field_class_object = fixture.shared.class_object(field_class).unwrap();

    assert_eq!(fixture.shared.get_class(field).unwrap(), field_class_object);

    let field_type = ref_value(
        run_instance_method(
            &fixture.shared,
            &fixture.thread,
            field_class,
            "getType",
            "()Ljava/lang/Class;",
            field,
            Vec::new(),
        )
        .unwrap(),
    );
    assert_eq!(field_type, comparator_object);
}

#[test]
fn declared_methods_are_stable_and_non_empty() {
    let fixture = fixture();
    let expected_methods: Vec<String> = {
        let entry = fixture.shared.entry(fixture.string_class).unwrap();
        entry
            .file
            .methods
            .iter()
            .filter_map(|method| {
                let name = entry.file.method_name(method).unwrap();
                (name != "<init>" && name != "<clinit>").then(|| name.to_owned())
            })
            .collect()
    };
    assert!(!expected_methods.is_empty(), "String declares methods");

    let count = int_value(
        call(
            &fixture,
            "methodCount",
            "(Ljava/lang/Class;)I",
            vec![string_class_argument(&fixture)],
        )
        .unwrap(),
    );
    assert_eq!(usize::try_from(count).unwrap(), expected_methods.len());

    let first = string_value(
        &fixture.shared,
        call(
            &fixture,
            "firstMethodName",
            "(Ljava/lang/Class;)Ljava/lang/String;",
            vec![string_class_argument(&fixture)],
        )
        .unwrap(),
    );
    assert_eq!(first, expected_methods[0]);

    let first_again = string_value(
        &fixture.shared,
        call(
            &fixture,
            "firstMethodName",
            "(Ljava/lang/Class;)Ljava/lang/String;",
            vec![string_class_argument(&fixture)],
        )
        .unwrap(),
    );
    assert_eq!(first, first_again, "member names are stable across calls");

    let runtime = string_value(
        &fixture.shared,
        call(
            &fixture,
            "methodRuntimeClassName",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
            vec![
                string_class_argument(&fixture),
                name_argument(&fixture, "concat"),
            ],
        )
        .unwrap(),
    );
    assert_eq!(runtime, "java.lang.reflect.Method");

    let parameter_count = int_value(
        call(
            &fixture,
            "methodParameterCount",
            "(Ljava/lang/Class;Ljava/lang/String;)I",
            vec![
                string_class_argument(&fixture),
                name_argument(&fixture, "concat"),
            ],
        )
        .unwrap(),
    );
    assert_eq!(parameter_count, 1);

    let parameter_type = string_value(
        &fixture.shared,
        call(
            &fixture,
            "methodParameterTypeName",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
            vec![
                string_class_argument(&fixture),
                name_argument(&fixture, "concat"),
            ],
        )
        .unwrap(),
    );
    assert_eq!(parameter_type, "java.lang.String");

    let return_type = string_value(
        &fixture.shared,
        call(
            &fixture,
            "methodReturnTypeName",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
            vec![
                string_class_argument(&fixture),
                name_argument(&fixture, "concat"),
            ],
        )
        .unwrap(),
    );
    assert_eq!(return_type, "java.lang.String");

    let modifiers = int_value(
        call(
            &fixture,
            "methodModifiers",
            "(Ljava/lang/Class;Ljava/lang/String;)I",
            vec![
                string_class_argument(&fixture),
                name_argument(&fixture, "concat"),
            ],
        )
        .unwrap(),
    );
    assert_eq!(modifiers, 0x0001, "concat is public");

    let declaring = string_value(
        &fixture.shared,
        call(
            &fixture,
            "methodDeclaringClassName",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
            vec![
                string_class_argument(&fixture),
                name_argument(&fixture, "concat"),
            ],
        )
        .unwrap(),
    );
    assert_eq!(declaring, "java.lang.String");

    // The method array itself is a real reference array of reflected `Method` objects.
    let array = ref_value(
        call(
            &fixture,
            "declaredMethods",
            "(Ljava/lang/Class;)[Ljava/lang/reflect/Method;",
            vec![string_class_argument(&fixture)],
        )
        .unwrap(),
    );
    let elements = fixture.shared.heap().array(array).unwrap().elements.clone();
    assert_eq!(elements.len(), expected_methods.len());
    for value in &elements {
        let Value::Ref(reference) = value else {
            panic!("a declared-method array holds references, found {value:?}");
        };
        assert_eq!(
            fixture.shared.heap().reflect(*reference).unwrap().kind,
            MemberKind::Method
        );
    }
}

#[test]
fn declared_constructors_report_names_and_parameter_counts() {
    let fixture = fixture();
    let entry = fixture.shared.entry(fixture.string_class).unwrap();
    let constructors: Vec<&rava_classfile::Method> = entry
        .file
        .methods
        .iter()
        .filter(|method| entry.file.method_name(method).unwrap() == "<init>")
        .collect();
    assert!(!constructors.is_empty(), "String declares constructors");
    let expected_count = constructors.len();
    let expected_parameters =
        parameter_count(entry.file.method_descriptor(constructors[0]).unwrap());

    let count = int_value(
        call(
            &fixture,
            "constructorCount",
            "(Ljava/lang/Class;)I",
            vec![string_class_argument(&fixture)],
        )
        .unwrap(),
    );
    assert_eq!(usize::try_from(count).unwrap(), expected_count);

    let parameters = int_value(
        call(
            &fixture,
            "firstConstructorParameterCount",
            "(Ljava/lang/Class;)I",
            vec![string_class_argument(&fixture)],
        )
        .unwrap(),
    );
    assert_eq!(usize::try_from(parameters).unwrap(), expected_parameters);

    let name = string_value(
        &fixture.shared,
        call(
            &fixture,
            "firstConstructorName",
            "(Ljava/lang/Class;)Ljava/lang/String;",
            vec![string_class_argument(&fixture)],
        )
        .unwrap(),
    );
    assert_eq!(
        name, "java.lang.String",
        "Constructor.getName is the class name"
    );

    let runtime = string_value(
        &fixture.shared,
        call(
            &fixture,
            "firstConstructorRuntimeClassName",
            "(Ljava/lang/Class;)Ljava/lang/String;",
            vec![string_class_argument(&fixture)],
        )
        .unwrap(),
    );
    assert_eq!(runtime, "java.lang.reflect.Constructor");

    let declaring = string_value(
        &fixture.shared,
        call(
            &fixture,
            "firstConstructorDeclaringClassName",
            "(Ljava/lang/Class;)Ljava/lang/String;",
            vec![string_class_argument(&fixture)],
        )
        .unwrap(),
    );
    assert_eq!(declaring, "java.lang.String");

    let array = ref_value(
        call(
            &fixture,
            "declaredConstructors",
            "(Ljava/lang/Class;)[Ljava/lang/reflect/Constructor;",
            vec![string_class_argument(&fixture)],
        )
        .unwrap(),
    );
    let elements = fixture.shared.heap().array(array).unwrap().elements.clone();
    assert_eq!(elements.len(), expected_count);
    for value in &elements {
        let Value::Ref(reference) = value else {
            panic!("a declared-constructor array holds references, found {value:?}");
        };
        let member = fixture.shared.heap().reflect(*reference).unwrap().clone();
        assert_eq!(member.kind, MemberKind::Constructor);
        assert_eq!(member.name, "<init>");
    }
}

#[test]
fn probe_declared_fields_keep_class_file_order_and_find_by_name() {
    let fixture = fixture();
    let probe_object = fixture.shared.class_object(fixture.probe).unwrap();

    let array = ref_value(
        call(
            &fixture,
            "declaredFields",
            "(Ljava/lang/Class;)[Ljava/lang/reflect/Field;",
            vec![Value::Ref(probe_object)],
        )
        .unwrap(),
    );
    let names: Vec<String> = {
        let heap = fixture.shared.heap();
        let elements = heap.array(array).unwrap().elements.clone();
        let mut names = Vec::new();
        for value in elements {
            let Value::Ref(reference) = value else {
                panic!("a declared-field array holds references, found {value:?}");
            };
            names.push(heap.reflect(reference).unwrap().name.clone());
        }
        names
    };
    assert_eq!(names, ["counter", "label", "other"]);

    let found = string_value(
        &fixture.shared,
        call(
            &fixture,
            "fieldName",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
            vec![Value::Ref(probe_object), name_argument(&fixture, "label")],
        )
        .unwrap(),
    );
    assert_eq!(found, "label");

    let type_name = string_value(
        &fixture.shared,
        call(
            &fixture,
            "fieldTypeName",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
            vec![Value::Ref(probe_object), name_argument(&fixture, "label")],
        )
        .unwrap(),
    );
    assert_eq!(type_name, "java.lang.String");

    let modifiers = int_value(
        call(
            &fixture,
            "fieldModifiers",
            "(Ljava/lang/Class;Ljava/lang/String;)I",
            vec![Value::Ref(probe_object), name_argument(&fixture, "label")],
        )
        .unwrap(),
    );
    assert_eq!(modifiers, 0x0009, "public static");
}

#[test]
fn primitive_and_array_descriptors_are_typed_errors() {
    let fixture = fixture();

    // `Integer.MAX_VALUE` is an `int`; `String.charAt` takes an `int`.
    let integer_class = fixture
        .shared
        .load_library_class("java/lang/Integer")
        .unwrap();
    let integer_object = fixture.shared.class_object(integer_class).unwrap();
    let max_value = call(
        &fixture,
        "fieldTypeName",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
        vec![
            Value::Ref(integer_object),
            name_argument(&fixture, "MAX_VALUE"),
        ],
    );
    assert!(
        matches!(
            max_value,
            Err(InterpError::Vm(VmError::Native(ref detail)))
                if detail.contains("primitive class objects")
        ),
        "{max_value:?}"
    );

    let char_at = call(
        &fixture,
        "methodParameterTypeName",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
        vec![
            string_class_argument(&fixture),
            name_argument(&fixture, "charAt"),
        ],
    );
    assert!(
        matches!(
            char_at,
            Err(InterpError::Vm(VmError::Native(ref detail)))
                if detail.contains("primitive class objects")
        ),
        "{char_at:?}"
    );
}

#[test]
fn inherited_member_queries_are_typed_unsupported() {
    let fixture = fixture();
    let class_class = fixture
        .shared
        .load_library_class("java/lang/Class")
        .unwrap();

    let fields = run_instance_method(
        &fixture.shared,
        &fixture.thread,
        class_class,
        "getFields",
        "()[Ljava/lang/reflect/Field;",
        fixture.string_object,
        Vec::new(),
    );
    assert!(
        matches!(
            fields,
            Err(InterpError::NativeOrAbstract { ref name, .. }) if name == "getFields"
        ),
        "{fields:?}"
    );

    let field = run_instance_method(
        &fixture.shared,
        &fixture.thread,
        class_class,
        "getDeclaredField",
        "(Ljava/lang/String;)Ljava/lang/reflect/Field;",
        fixture.string_object,
        vec![name_argument(&fixture, "value")],
    );
    assert!(
        matches!(
            field,
            Err(InterpError::NativeOrAbstract { ref name, .. }) if name == "getDeclaredField"
        ),
        "{field:?}"
    );
}
