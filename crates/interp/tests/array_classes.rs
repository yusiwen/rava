//! M2.16 acceptance: array `Class` identity and array member types.
//!
//! `Bean` is an ordinary class with an `int[]`, a `String[]` and an `int[][]` field, a
//! `()V`, an `([I)V` and an `([I[Ljava/lang/String;)V` constructor, and methods that
//! take and return `int[]` and `String[]`. `ArrayClassProbe` is a bytecode client, so
//! `Class.forName`, `Object.getClass`, the `Class` queries and the named lookups run
//! through the interpreter's virtual-dispatch path rather than a hand-built native call.
//!
//! The assertions cover the JDK shape: an array class's `getName` is the descriptor form
//! (`[I`, `[[I`, `[Ljava.lang.String;`), `isArray` is true, `isPrimitive` is false,
//! `getModifiers` is `0x0411` and `getSuperclass` is `java.lang.Object`; a component is a
//! primitive class object, a named class object or a nested array class object. The
//! documented boundaries are asserted too: a malformed array descriptor and a named
//! component that the bootstrap loader does not define are typed errors, an array class
//! declares no members, and a lookup miss is null.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_instance_method, run_static_method};
use rava_vm::{
    ClassId, ClassRef, LoaderId, MemberKind, ObjRef, PrimitiveKind, Shared, ThreadState, Value,
    VmError, instance, object, reflection,
};

const ACC_PRIVATE: u16 = 0x0002;
const ACC_PUBLIC: u16 = 0x0001;
const ACC_PUBLIC_STATIC: u16 = 0x0009;

/// `Modifier.PUBLIC | Modifier.ABSTRACT | Modifier.FINAL`, the JDK's array modifiers.
const ARRAY_MODIFIERS: i32 = 0x0411;

const CLASS_NAME: &str = "()Ljava/lang/String;";
const CLASS_MODIFIERS: &str = "()I";
const CLASS_IS_ARRAY: &str = "()Z";
const CLASS_IS_PRIMITIVE: &str = "()Z";
const CLASS_COMPONENT_TYPE: &str = "()Ljava/lang/Class;";
const CLASS_SUPERCLASS: &str = "()Ljava/lang/Class;";
const FIELD_GET_TYPE: &str = "()Ljava/lang/Class;";
const METHOD_GET_RETURN_TYPE: &str = "()Ljava/lang/Class;";
const PARAMETER_TYPES: &str = "()[Ljava/lang/Class;";
const MEMBER_GET_PARAMETER_COUNT: &str = "()I";
const DECLARED_METHOD: &str = "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;";
const DECLARED_FIELD: &str = "(Ljava/lang/String;)Ljava/lang/reflect/Field;";
const DECLARED_CONSTRUCTOR: &str = "([Ljava/lang/Class;)Ljava/lang/reflect/Constructor;";
const DECLARED_FIELDS: &str = "()[Ljava/lang/reflect/Field;";

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn static_method(
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

fn instance_method(
    builder: &mut ClassBuilder,
    access: u16,
    name: &str,
    descriptor: &str,
    max_stack: u16,
    max_locals: u16,
    body: Assembler,
) {
    builder.method(
        access,
        name,
        descriptor,
        Some((max_stack, max_locals, body.finish().unwrap())),
    );
}

/// `Bean`: the fixture class whose array members and constructors the tests reflect over.
fn bean_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("Bean", "java/lang/Object", 61);
    builder.field(ACC_PRIVATE, "values", "[I");
    builder.field(ACC_PRIVATE, "labels", "[Ljava/lang/String;");
    builder.field(ACC_PRIVATE, "grid", "[[I");

    let object_init = builder.method_ref("java/lang/Object", "<init>", "()V");
    let values_ref = builder.field_ref("Bean", "values", "[I");
    let labels_ref = builder.field_ref("Bean", "labels", "[Ljava/lang/String;");
    let grid_ref = builder.field_ref("Bean", "grid", "[[I");

    // Bean() { super(); }
    let mut init = Assembler::new();
    init.aload(0);
    init.invokespecial(object_init);
    init.return_();
    instance_method(&mut builder, ACC_PUBLIC, "<init>", "()V", 1, 1, init);

    // Bean(int[] values) { super(); this.values = values; }
    let mut init_values = Assembler::new();
    init_values.aload(0);
    init_values.invokespecial(object_init);
    init_values.aload(0);
    init_values.aload(1);
    init_values.putfield(values_ref);
    init_values.return_();
    instance_method(
        &mut builder,
        ACC_PUBLIC,
        "<init>",
        "([I)V",
        2,
        2,
        init_values,
    );

    // Bean(int[] values, String[] labels) { super(); this.values = values;
    //     this.labels = labels; }
    let mut init_both = Assembler::new();
    init_both.aload(0);
    init_both.invokespecial(object_init);
    init_both.aload(0);
    init_both.aload(1);
    init_both.putfield(values_ref);
    init_both.aload(0);
    init_both.aload(2);
    init_both.putfield(labels_ref);
    init_both.return_();
    instance_method(
        &mut builder,
        ACC_PUBLIC,
        "<init>",
        "([I[Ljava/lang/String;)V",
        2,
        3,
        init_both,
    );

    // int[] values() { return this.values; }
    let mut get_values = Assembler::new();
    get_values.aload(0);
    get_values.getfield(values_ref);
    get_values.areturn();
    instance_method(&mut builder, ACC_PUBLIC, "values", "()[I", 1, 1, get_values);

    // void setValues(int[] values) { this.values = values; }
    let mut set_values = Assembler::new();
    set_values.aload(0);
    set_values.aload(1);
    set_values.putfield(values_ref);
    set_values.return_();
    instance_method(
        &mut builder,
        ACC_PUBLIC,
        "setValues",
        "([I)V",
        2,
        2,
        set_values,
    );

    // String[] labels() { return this.labels; }
    let mut get_labels = Assembler::new();
    get_labels.aload(0);
    get_labels.getfield(labels_ref);
    get_labels.areturn();
    instance_method(
        &mut builder,
        ACC_PUBLIC,
        "labels",
        "()[Ljava/lang/String;",
        1,
        1,
        get_labels,
    );

    // int[][] grid() { return this.grid; }
    let mut get_grid = Assembler::new();
    get_grid.aload(0);
    get_grid.getfield(grid_ref);
    get_grid.areturn();
    instance_method(&mut builder, ACC_PUBLIC, "grid", "()[[I", 1, 1, get_grid);

    // int[] echo(int[] values) { return values; }
    let mut echo = Assembler::new();
    echo.aload(1);
    echo.areturn();
    instance_method(&mut builder, ACC_PUBLIC, "echo", "([I)[I", 1, 2, echo);

    // String[] echoLabels(String[] labels) { return labels; }
    let mut echo_labels = Assembler::new();
    echo_labels.aload(1);
    echo_labels.areturn();
    instance_method(
        &mut builder,
        ACC_PUBLIC,
        "echoLabels",
        "([Ljava/lang/String;)[Ljava/lang/String;",
        1,
        2,
        echo_labels,
    );

    // int count(int[] values) { return values.length; }
    let mut count = Assembler::new();
    count.aload(1);
    count.arraylength();
    count.ireturn();
    instance_method(&mut builder, ACC_PUBLIC, "count", "([I)I", 1, 2, count);

    builder.build()
}

/// `ArrayClassProbe`: a bytecode client for the array `Class` surface.
fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("ArrayClassProbe", "java/lang/Object", 61);
    let object_class = builder.class("java/lang/Object");
    let string_class = builder.class("java/lang/String");
    let int_grid_class = builder.class("[[I");
    let object_get_class =
        builder.method_ref("java/lang/Object", "getClass", "()Ljava/lang/Class;");
    let class_get_name = builder.method_ref("java/lang/Class", "getName", CLASS_NAME);
    let class_for_name = builder.method_ref(
        "java/lang/Class",
        "forName",
        "(Ljava/lang/String;)Ljava/lang/Class;",
    );
    let class_get_modifiers =
        builder.method_ref("java/lang/Class", "getModifiers", CLASS_MODIFIERS);
    let class_is_array = builder.method_ref("java/lang/Class", "isArray", CLASS_IS_ARRAY);
    let class_is_primitive =
        builder.method_ref("java/lang/Class", "isPrimitive", CLASS_IS_PRIMITIVE);
    let class_get_component_type =
        builder.method_ref("java/lang/Class", "getComponentType", CLASS_COMPONENT_TYPE);
    let class_get_superclass =
        builder.method_ref("java/lang/Class", "getSuperclass", CLASS_SUPERCLASS);
    let class_get_declared_method =
        builder.method_ref("java/lang/Class", "getDeclaredMethod", DECLARED_METHOD);
    let class_get_declared_field =
        builder.method_ref("java/lang/Class", "getDeclaredField", DECLARED_FIELD);
    let class_get_declared_constructor = builder.method_ref(
        "java/lang/Class",
        "getDeclaredConstructor",
        DECLARED_CONSTRUCTOR,
    );
    let field_get_type = builder.method_ref("java/lang/reflect/Field", "getType", FIELD_GET_TYPE);
    let method_get_return_type = builder.method_ref(
        "java/lang/reflect/Method",
        "getReturnType",
        METHOD_GET_RETURN_TYPE,
    );
    let method_get_parameter_types = builder.method_ref(
        "java/lang/reflect/Method",
        "getParameterTypes",
        PARAMETER_TYPES,
    );
    let constructor_get_parameter_types = builder.method_ref(
        "java/lang/reflect/Constructor",
        "getParameterTypes",
        PARAMETER_TYPES,
    );

    // static Class forNameClass(String name) { return Class.forName(name); }
    let mut for_name = Assembler::new();
    for_name.aload(0);
    for_name.op(0xb8, &class_for_name.to_be_bytes()); // invokestatic
    for_name.areturn();
    static_method(
        &mut builder,
        "forNameClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        1,
        1,
        for_name,
    );

    // static String name(Class c) { return c.getName(); }
    let mut name = Assembler::new();
    name.aload(0);
    name.invokevirtual(class_get_name);
    name.areturn();
    static_method(
        &mut builder,
        "name",
        "(Ljava/lang/Class;)Ljava/lang/String;",
        1,
        1,
        name,
    );

    // static int modifiers(Class c) { return c.getModifiers(); }
    let mut modifiers = Assembler::new();
    modifiers.aload(0);
    modifiers.invokevirtual(class_get_modifiers);
    modifiers.ireturn();
    static_method(
        &mut builder,
        "modifiers",
        "(Ljava/lang/Class;)I",
        1,
        1,
        modifiers,
    );

    // static boolean isArray(Class c) { return c.isArray(); }
    let mut is_array = Assembler::new();
    is_array.aload(0);
    is_array.invokevirtual(class_is_array);
    is_array.ireturn();
    static_method(
        &mut builder,
        "isArray",
        "(Ljava/lang/Class;)Z",
        1,
        1,
        is_array,
    );

    // static boolean isPrimitive(Class c) { return c.isPrimitive(); }
    let mut is_primitive = Assembler::new();
    is_primitive.aload(0);
    is_primitive.invokevirtual(class_is_primitive);
    is_primitive.ireturn();
    static_method(
        &mut builder,
        "isPrimitive",
        "(Ljava/lang/Class;)Z",
        1,
        1,
        is_primitive,
    );

    // static Class componentType(Class c) { return c.getComponentType(); }
    let mut component_type = Assembler::new();
    component_type.aload(0);
    component_type.invokevirtual(class_get_component_type);
    component_type.areturn();
    static_method(
        &mut builder,
        "componentType",
        "(Ljava/lang/Class;)Ljava/lang/Class;",
        1,
        1,
        component_type,
    );

    // static Class superclass(Class c) { return c.getSuperclass(); }
    let mut superclass = Assembler::new();
    superclass.aload(0);
    superclass.invokevirtual(class_get_superclass);
    superclass.areturn();
    static_method(
        &mut builder,
        "superclass",
        "(Ljava/lang/Class;)Ljava/lang/Class;",
        1,
        1,
        superclass,
    );

    // static String runtimeName(Object o) { return o.getClass().getName(); }
    let mut runtime_name = Assembler::new();
    runtime_name.aload(0);
    runtime_name.invokevirtual(object_get_class);
    runtime_name.invokevirtual(class_get_name);
    runtime_name.areturn();
    static_method(
        &mut builder,
        "runtimeName",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        2,
        1,
        runtime_name,
    );

    // static Class runtimeClass(Object o) { return o.getClass(); }
    let mut runtime_class = Assembler::new();
    runtime_class.aload(0);
    runtime_class.invokevirtual(object_get_class);
    runtime_class.areturn();
    static_method(
        &mut builder,
        "runtimeClass",
        "(Ljava/lang/Object;)Ljava/lang/Class;",
        1,
        1,
        runtime_class,
    );

    // static int[] makeInts(int n) { return new int[n]; }
    let mut make_ints = Assembler::new();
    make_ints.iload(0);
    make_ints.newarray(10); // T_INT
    make_ints.areturn();
    static_method(&mut builder, "makeInts", "(I)[I", 1, 1, make_ints);

    // static String[] makeStrings(int n) { return new String[n]; }
    let mut make_strings = Assembler::new();
    make_strings.iload(0);
    make_strings.anewarray(string_class);
    make_strings.areturn();
    static_method(
        &mut builder,
        "makeStrings",
        "(I)[Ljava/lang/String;",
        1,
        1,
        make_strings,
    );

    // static Object[] makeObjects(int n) { return new Object[n]; }
    let mut make_objects = Assembler::new();
    make_objects.iload(0);
    make_objects.anewarray(object_class);
    make_objects.areturn();
    static_method(
        &mut builder,
        "makeObjects",
        "(I)[Ljava/lang/Object;",
        1,
        1,
        make_objects,
    );

    // static int[][] makeGrid(int rows, int cols) { return new int[rows][cols]; }
    let mut make_grid = Assembler::new();
    make_grid.iload(0);
    make_grid.iload(1);
    make_grid.multianewarray(int_grid_class, 2);
    make_grid.areturn();
    static_method(&mut builder, "makeGrid", "(II)[[I", 2, 2, make_grid);

    // static int[][] makeRows(int n) { return new int[n][]; }
    let mut make_rows = Assembler::new();
    make_rows.iload(0);
    make_rows.multianewarray(int_grid_class, 1);
    make_rows.areturn();
    static_method(&mut builder, "makeRows", "(I)[[I", 1, 1, make_rows);

    // static int length(Object array) { return ((int[]) array).length; }
    // The cast is implicit: `arraylength` reads any array reference.
    let mut length = Assembler::new();
    length.aload(0);
    length.arraylength();
    length.ireturn();
    static_method(
        &mut builder,
        "length",
        "(Ljava/lang/Object;)I",
        1,
        1,
        length,
    );

    // static Method declaredMethod(Class c, String name, Class[] params) {
    //     return c.getDeclaredMethod(name, params);
    // }
    let mut declared_method = Assembler::new();
    declared_method.aload(0);
    declared_method.aload(1);
    declared_method.aload(2);
    declared_method.invokevirtual(class_get_declared_method);
    declared_method.areturn();
    static_method(
        &mut builder,
        "declaredMethod",
        "(Ljava/lang/Class;Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;",
        3,
        3,
        declared_method,
    );

    // static Constructor declaredConstructor(Class c, Class[] params) {
    //     return c.getDeclaredConstructor(params);
    // }
    let mut declared_constructor = Assembler::new();
    declared_constructor.aload(0);
    declared_constructor.aload(1);
    declared_constructor.invokevirtual(class_get_declared_constructor);
    declared_constructor.areturn();
    static_method(
        &mut builder,
        "declaredConstructor",
        "(Ljava/lang/Class;[Ljava/lang/Class;)Ljava/lang/reflect/Constructor;",
        2,
        2,
        declared_constructor,
    );

    // static String fieldTypeName(Class c, String name) {
    //     return c.getDeclaredField(name).getType().getName();
    // }
    let mut field_type_name = Assembler::new();
    field_type_name.aload(0);
    field_type_name.aload(1);
    field_type_name.invokevirtual(class_get_declared_field);
    field_type_name.invokevirtual(field_get_type);
    field_type_name.invokevirtual(class_get_name);
    field_type_name.areturn();
    static_method(
        &mut builder,
        "fieldTypeName",
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
        2,
        2,
        field_type_name,
    );

    // static String methodReturnTypeName(Class c, String name, Class[] params) {
    //     return c.getDeclaredMethod(name, params).getReturnType().getName();
    // }
    let mut return_type_name = Assembler::new();
    return_type_name.aload(0);
    return_type_name.aload(1);
    return_type_name.aload(2);
    return_type_name.invokevirtual(class_get_declared_method);
    return_type_name.invokevirtual(method_get_return_type);
    return_type_name.invokevirtual(class_get_name);
    return_type_name.areturn();
    static_method(
        &mut builder,
        "methodReturnTypeName",
        "(Ljava/lang/Class;Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/String;",
        3,
        3,
        return_type_name,
    );

    // static Class[] methodParameterTypes(Class c, String name, Class[] params) {
    //     return c.getDeclaredMethod(name, params).getParameterTypes();
    // }
    let mut parameter_types = Assembler::new();
    parameter_types.aload(0);
    parameter_types.aload(1);
    parameter_types.aload(2);
    parameter_types.invokevirtual(class_get_declared_method);
    parameter_types.invokevirtual(method_get_parameter_types);
    parameter_types.areturn();
    static_method(
        &mut builder,
        "methodParameterTypes",
        "(Ljava/lang/Class;Ljava/lang/String;[Ljava/lang/Class;)[Ljava/lang/Class;",
        3,
        3,
        parameter_types,
    );

    // static Class[] constructorParameterTypes(Class c, Class[] params) {
    //     return c.getDeclaredConstructor(params).getParameterTypes();
    // }
    let mut constructor_parameter_types = Assembler::new();
    constructor_parameter_types.aload(0);
    constructor_parameter_types.aload(1);
    constructor_parameter_types.invokevirtual(class_get_declared_constructor);
    constructor_parameter_types.invokevirtual(constructor_get_parameter_types);
    constructor_parameter_types.areturn();
    static_method(
        &mut builder,
        "constructorParameterTypes",
        "(Ljava/lang/Class;[Ljava/lang/Class;)[Ljava/lang/Class;",
        2,
        2,
        constructor_parameter_types,
    );

    // static String parameterTypeName(Class c, String name, Class[] params, int index) {
    //     return c.getDeclaredMethod(name, params).getParameterTypes()[index].getName();
    // }
    let mut parameter_type_name = Assembler::new();
    parameter_type_name.aload(0);
    parameter_type_name.aload(1);
    parameter_type_name.aload(2);
    parameter_type_name.invokevirtual(class_get_declared_method);
    parameter_type_name.invokevirtual(method_get_parameter_types);
    parameter_type_name.iload(3);
    parameter_type_name.aaload();
    parameter_type_name.invokevirtual(class_get_name);
    parameter_type_name.areturn();
    static_method(
        &mut builder,
        "parameterTypeName",
        "(Ljava/lang/Class;Ljava/lang/String;[Ljava/lang/Class;I)Ljava/lang/String;",
        4,
        4,
        parameter_type_name,
    );

    builder.build()
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    bean: ClassId,
    bean_object: ObjRef,
    probe: ClassId,
    class_class: ClassId,
    class_object: ObjRef,
    object_object: ObjRef,
    string_object: ObjRef,
    field_class: ClassId,
    method_class: ClassId,
    constructor_class: ClassId,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    instance::register_natives(&shared);
    object::register_natives(&shared);
    reflection::register_natives(&shared);

    let bean = shared
        .define_class(LoaderId::BOOTSTRAP, "Bean", &bean_bytes())
        .expect("Bean defines");
    let probe = shared
        .define_class(LoaderId::BOOTSTRAP, "ArrayClassProbe", &probe_bytes())
        .expect("ArrayClassProbe defines");

    let load = |name: &str| {
        shared
            .load_library_class(name)
            .unwrap_or_else(|error| panic!("{name} loads: {error}"))
    };
    let class_class = load("java/lang/Class");
    let class_object = shared
        .class_object(class_class)
        .expect("Class class object");
    let object_class = load("java/lang/Object");
    let object_object = shared
        .class_object(object_class)
        .expect("Object class object");
    let string_object = shared
        .class_object(load("java/lang/String"))
        .expect("String class object");
    let field_class = load("java/lang/reflect/Field");
    let method_class = load("java/lang/reflect/Method");
    let constructor_class = load("java/lang/reflect/Constructor");
    let bean_object = shared.class_object(bean).expect("Bean class object");

    let thread = shared.thread_state(1);
    Fixture {
        shared,
        thread,
        bean,
        bean_object,
        probe,
        class_class,
        class_object,
        object_object,
        string_object,
        field_class,
        method_class,
        constructor_class,
    }
}

fn class_ref(fixture: &Fixture) -> ClassRef {
    ClassRef::Named(fixture.class_class)
}

fn reference_array(shared: &Shared, component: ClassRef, values: &[ObjRef]) -> ObjRef {
    let mut heap = shared.heap();
    let array = heap.allocate_array(component, values.len());
    {
        let array = heap.array_mut(array).expect("a reference array");
        for (slot, value) in array.elements.iter_mut().zip(values) {
            *slot = Value::Ref(*value);
        }
    }
    array
}

fn array_elements(shared: &Shared, reference: ObjRef) -> Vec<Value> {
    shared
        .heap()
        .array(reference)
        .expect("a heap array")
        .elements
        .clone()
}

fn call_static(
    fixture: &Fixture,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        name,
        descriptor,
        args,
    )
}

fn call_class(
    fixture: &Fixture,
    receiver: ObjRef,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    run_instance_method(
        &fixture.shared,
        &fixture.thread,
        fixture.class_class,
        name,
        descriptor,
        receiver,
        args,
    )
}

fn call_member(
    fixture: &Fixture,
    owner: ClassId,
    receiver: ObjRef,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    run_instance_method(
        &fixture.shared,
        &fixture.thread,
        owner,
        name,
        descriptor,
        receiver,
        args,
    )
}

fn name_value(fixture: &Fixture, name: &str) -> Value {
    Value::Ref(fixture.shared.intern_string(name))
}

fn params(fixture: &Fixture, classes: &[ObjRef]) -> Value {
    Value::Ref(reference_array(
        &fixture.shared,
        class_ref(fixture),
        classes,
    ))
}

fn for_name(fixture: &Fixture, name: &str) -> Result<ObjRef, InterpError> {
    let value = call_static(
        fixture,
        "forNameClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        vec![name_value(fixture, name)],
    )?;
    Ok(ref_value(value))
}

fn array_class(fixture: &Fixture, name: &str) -> ObjRef {
    for_name(fixture, name).unwrap_or_else(|error| panic!("Class.forName({name:?}): {error}"))
}

fn member(fixture: &Fixture, kind: MemberKind, name: &str, descriptor: &str) -> ObjRef {
    fixture
        .shared
        .reflect_member(fixture.bean, kind, name, descriptor)
        .expect("the member object allocates")
}

fn declared_method(
    fixture: &Fixture,
    class: ObjRef,
    name: &str,
    parameters: &[ObjRef],
) -> Result<Option<Value>, InterpError> {
    call_class(
        fixture,
        class,
        "getDeclaredMethod",
        DECLARED_METHOD,
        vec![name_value(fixture, name), params(fixture, parameters)],
    )
}

fn declared_constructor(
    fixture: &Fixture,
    class: ObjRef,
    parameters: &[ObjRef],
) -> Result<Option<Value>, InterpError> {
    call_class(
        fixture,
        class,
        "getDeclaredConstructor",
        DECLARED_CONSTRUCTOR,
        vec![params(fixture, parameters)],
    )
}

fn ref_value(value: Option<Value>) -> ObjRef {
    match value {
        Some(Value::Ref(reference)) => reference,
        other => panic!("expected a reference result, found {other:?}"),
    }
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

fn bool_value(value: Option<Value>) -> bool {
    int_value(value) != 0
}

fn is_null(result: Result<Option<Value>, InterpError>) -> bool {
    match result.unwrap_or_else(|error| panic!("lookup failed: {error}")) {
        Some(Value::Ref(reference)) => reference.is_null(),
        other => panic!("expected a reference result, found {other:?}"),
    }
}

/// The member identity behind a reference returned by a lookup.
fn member_of(fixture: &Fixture, reference: ObjRef) -> rava_vm::ReflectMember {
    fixture
        .shared
        .heap()
        .reflect(reference)
        .expect("a reflected member")
        .clone()
}

fn assert_native_error(result: Result<Option<Value>, InterpError>, fragment: &str) {
    match result {
        Err(InterpError::Vm(VmError::Native(ref detail))) => assert!(
            detail.contains(fragment),
            "expected a native error containing {fragment:?}, found {detail:?}"
        ),
        other => panic!("expected a native error containing {fragment:?}, found {other:?}"),
    }
}

fn assert_loader_error(result: Result<Option<Value>, InterpError>, name: &str) {
    match result {
        Err(InterpError::Vm(VmError::ClassNotFound { name: found, .. })) => {
            assert_eq!(found, name, "the missing class is named");
        }
        other => panic!("expected a missing-class error for {name:?}, found {other:?}"),
    }
}

#[test]
fn array_class_names_flags_and_components_are_jdk_shaped() {
    let fixture = fixture();

    let int_array_ref = ClassRef::array(ClassRef::Primitive(PrimitiveKind::Int));
    let string_array_ref = ClassRef::array(ClassRef::Named(
        fixture
            .shared
            .load_library_class("java/lang/String")
            .unwrap(),
    ));
    let int_grid_ref = ClassRef::array(int_array_ref.clone());
    let string_grid_ref = ClassRef::array(string_array_ref.clone());

    let cases = [
        ("[I", "[I", int_array_ref.clone()),
        (
            "[Ljava.lang.String;",
            "[Ljava.lang.String;",
            string_array_ref.clone(),
        ),
        ("[[I", "[[I", int_grid_ref.clone()),
        (
            "[[Ljava.lang.String;",
            "[[Ljava.lang.String;",
            string_grid_ref.clone(),
        ),
    ];

    for (query, expected_name, expected_class_ref) in cases {
        let first = array_class(&fixture, query);
        let stored = fixture.shared.heap().array_class(first).unwrap().clone();
        assert_eq!(
            ClassRef::Array(Box::new(stored)),
            expected_class_ref,
            "Class.forName({query:?}) holds the expected component"
        );

        let again = array_class(&fixture, query);
        assert_eq!(again, first, "a second forName returns the same object");
        assert_eq!(
            fixture
                .shared
                .array_class(match expected_class_ref.clone() {
                    ClassRef::Array(array) => array.component,
                    _ => unreachable!("the case table holds array refs"),
                })
                .unwrap(),
            first,
            "the array-class cache stores this object under its component"
        );

        assert_eq!(
            string_value(
                &fixture.shared,
                call_static(
                    &fixture,
                    "name",
                    "(Ljava/lang/Class;)Ljava/lang/String;",
                    vec![Value::Ref(first)],
                )
                .unwrap()
            ),
            expected_name,
            "Class.getName"
        );

        assert!(
            bool_value(
                call_static(
                    &fixture,
                    "isArray",
                    "(Ljava/lang/Class;)Z",
                    vec![Value::Ref(first)],
                )
                .unwrap()
            ),
            "isArray for {query}"
        );
        assert!(
            !bool_value(
                call_static(
                    &fixture,
                    "isPrimitive",
                    "(Ljava/lang/Class;)Z",
                    vec![Value::Ref(first)],
                )
                .unwrap()
            ),
            "isPrimitive for {query}"
        );
        assert_eq!(
            int_value(
                call_static(
                    &fixture,
                    "modifiers",
                    "(Ljava/lang/Class;)I",
                    vec![Value::Ref(first)],
                )
                .unwrap()
            ),
            ARRAY_MODIFIERS,
            "array modifiers for {query}"
        );

        let component = ref_value(
            call_static(
                &fixture,
                "componentType",
                "(Ljava/lang/Class;)Ljava/lang/Class;",
                vec![Value::Ref(first)],
            )
            .unwrap(),
        );
        let expected_component = match &expected_class_ref {
            ClassRef::Array(array) => array.component.clone(),
            _ => unreachable!("the case table holds array refs"),
        };
        assert_eq!(
            component,
            fixture
                .shared
                .class_object_for_ref(&expected_component)
                .unwrap(),
            "getComponentType for {query}"
        );

        let superclass = ref_value(
            call_static(
                &fixture,
                "superclass",
                "(Ljava/lang/Class;)Ljava/lang/Class;",
                vec![Value::Ref(first)],
            )
            .unwrap(),
        );
        assert_eq!(
            superclass, fixture.object_object,
            "getSuperclass for {query} is java.lang.Object"
        );

        assert_eq!(
            string_value(
                &fixture.shared,
                call_static(
                    &fixture,
                    "runtimeName",
                    "(Ljava/lang/Object;)Ljava/lang/String;",
                    vec![Value::Ref(first)],
                )
                .unwrap()
            ),
            "java.lang.Class",
            "getClass() on an array class object reports java.lang.Class"
        );
        assert_eq!(
            fixture.shared.get_class(first).unwrap(),
            fixture.class_object,
            "Shared::get_class agrees with getClass on the array class object"
        );

        assert!(
            fixture.shared.roots(&[]).contains(&first),
            "the array class object is a root"
        );
    }

    // `Class.forName` also accepts the internal descriptor spelling, and both spellings
    // share one cached object.
    assert_eq!(
        array_class(&fixture, "[Ljava/lang/String;"),
        array_class(&fixture, "[Ljava/lang/String;")
    );

    // The component chain of `[[I` is `[I` then `int`.
    let int_array_object = fixture.shared.class_object_for_ref(&int_array_ref).unwrap();
    let int_grid_object = fixture.shared.class_object_for_ref(&int_grid_ref).unwrap();
    let grid_component = ref_value(
        call_static(
            &fixture,
            "componentType",
            "(Ljava/lang/Class;)Ljava/lang/Class;",
            vec![Value::Ref(int_grid_object)],
        )
        .unwrap(),
    );
    assert_eq!(grid_component, int_array_object);
    assert_eq!(
        ref_value(
            call_static(
                &fixture,
                "componentType",
                "(Ljava/lang/Class;)Ljava/lang/Class;",
                vec![Value::Ref(grid_component)],
            )
            .unwrap()
        ),
        fixture.shared.primitive_class(PrimitiveKind::Int)
    );
}

#[test]
fn get_class_of_new_arrays_returns_the_cached_array_class() {
    let fixture = fixture();

    let ints = ref_value(call_static(&fixture, "makeInts", "(I)[I", vec![Value::Int(3)]).unwrap());
    assert_eq!(
        fixture.shared.heap().array(ints).unwrap().component,
        ClassRef::Primitive(PrimitiveKind::Int)
    );
    let int_array = array_class(&fixture, "[I");
    assert_eq!(
        fixture.shared.get_class(ints).unwrap(),
        int_array,
        "getClass() on new int[3] is int[]"
    );
    assert_eq!(
        ref_value(
            call_static(
                &fixture,
                "runtimeClass",
                "(Ljava/lang/Object;)Ljava/lang/Class;",
                vec![Value::Ref(ints)],
            )
            .unwrap()
        ),
        int_array
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            call_static(
                &fixture,
                "runtimeName",
                "(Ljava/lang/Object;)Ljava/lang/String;",
                vec![Value::Ref(ints)],
            )
            .unwrap()
        ),
        "[I"
    );
    assert_eq!(
        int_value(
            call_static(
                &fixture,
                "length",
                "(Ljava/lang/Object;)I",
                vec![Value::Ref(ints)],
            )
            .unwrap()
        ),
        3,
        "arraylength still works on the array"
    );

    let strings = ref_value(
        call_static(
            &fixture,
            "makeStrings",
            "(I)[Ljava/lang/String;",
            vec![Value::Int(2)],
        )
        .unwrap(),
    );
    assert_eq!(
        fixture.shared.heap().array(strings).unwrap().component,
        ClassRef::Named(
            fixture
                .shared
                .load_library_class("java/lang/String")
                .unwrap()
        )
    );
    let string_array = array_class(&fixture, "[Ljava.lang.String;");
    assert_eq!(
        fixture.shared.get_class(strings).unwrap(),
        string_array,
        "getClass() on new String[2] is String[]"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            call_static(
                &fixture,
                "runtimeName",
                "(Ljava/lang/Object;)Ljava/lang/String;",
                vec![Value::Ref(strings)],
            )
            .unwrap()
        ),
        "[Ljava.lang.String;"
    );

    let objects = ref_value(
        call_static(
            &fixture,
            "makeObjects",
            "(I)[Ljava/lang/Object;",
            vec![Value::Int(1)],
        )
        .unwrap(),
    );
    assert_eq!(
        fixture.shared.get_class(objects).unwrap(),
        array_class(&fixture, "[Ljava.lang.Object;")
    );

    // `multianewarray` of `[[I` carries `[I` as the outer component and `int` as the
    // inner one.
    let grid = ref_value(
        call_static(
            &fixture,
            "makeGrid",
            "(II)[[I",
            vec![Value::Int(2), Value::Int(3)],
        )
        .unwrap(),
    );
    let int_grid = array_class(&fixture, "[[I");
    assert_eq!(fixture.shared.get_class(grid).unwrap(), int_grid);
    assert_eq!(
        fixture.shared.heap().array(grid).unwrap().component,
        ClassRef::array(ClassRef::Primitive(PrimitiveKind::Int))
    );
    {
        let heap = fixture.shared.heap();
        let outer = heap.array(grid).expect("a grid");
        assert_eq!(outer.kind, rava_vm::ArrayKind::Reference);
        assert_eq!(outer.elements.len(), 2);
        for row in &outer.elements {
            let Value::Ref(row) = row else {
                panic!("a row is a reference, found {row:?}");
            };
            let row = heap.array(*row).expect("a row is an array");
            assert_eq!(row.kind, rava_vm::ArrayKind::Int);
            assert_eq!(row.component, ClassRef::Primitive(PrimitiveKind::Int));
        }
    }

    // `new int[n][]` allocates only the outer dimension; it is still `int[][]`.
    let rows = ref_value(call_static(&fixture, "makeRows", "(I)[[I", vec![Value::Int(2)]).unwrap());
    assert_eq!(
        fixture.shared.get_class(rows).unwrap(),
        int_grid,
        "a partially allocated int[][] is still the int[][] class"
    );
    assert_eq!(
        array_elements(&fixture.shared, rows),
        vec![Value::Ref(ObjRef::NULL); 2]
    );
}

#[test]
fn array_member_types_are_array_class_objects() {
    let fixture = fixture();
    let int_array = array_class(&fixture, "[I");
    let string_array = array_class(&fixture, "[Ljava.lang.String;");
    let int_grid = array_class(&fixture, "[[I");

    // Fields.
    let values = member(&fixture, MemberKind::Field, "values", "[I");
    assert_eq!(
        ref_value(
            call_member(
                &fixture,
                fixture.field_class,
                values,
                "getType",
                FIELD_GET_TYPE,
                Vec::new(),
            )
            .unwrap()
        ),
        int_array,
        "Field.getType of values:[I"
    );
    let labels = member(&fixture, MemberKind::Field, "labels", "[Ljava/lang/String;");
    assert_eq!(
        ref_value(
            call_member(
                &fixture,
                fixture.field_class,
                labels,
                "getType",
                FIELD_GET_TYPE,
                Vec::new(),
            )
            .unwrap()
        ),
        string_array,
        "Field.getType of labels:[Ljava/lang/String;"
    );
    let grid = member(&fixture, MemberKind::Field, "grid", "[[I");
    assert_eq!(
        ref_value(
            call_member(
                &fixture,
                fixture.field_class,
                grid,
                "getType",
                FIELD_GET_TYPE,
                Vec::new(),
            )
            .unwrap()
        ),
        int_grid,
        "Field.getType of grid:[[I"
    );

    // Methods.
    let echo = member(&fixture, MemberKind::Method, "echo", "([I)[I");
    assert_eq!(
        ref_value(
            call_member(
                &fixture,
                fixture.method_class,
                echo,
                "getReturnType",
                METHOD_GET_RETURN_TYPE,
                Vec::new(),
            )
            .unwrap()
        ),
        int_array,
        "Method.getReturnType of echo"
    );
    assert_eq!(
        array_elements(
            &fixture.shared,
            ref_value(
                call_member(
                    &fixture,
                    fixture.method_class,
                    echo,
                    "getParameterTypes",
                    PARAMETER_TYPES,
                    Vec::new(),
                )
                .unwrap()
            )
        ),
        [Value::Ref(int_array)],
        "Method.getParameterTypes of echo"
    );
    assert_eq!(
        int_value(
            call_member(
                &fixture,
                fixture.method_class,
                echo,
                "getParameterCount",
                MEMBER_GET_PARAMETER_COUNT,
                Vec::new(),
            )
            .unwrap()
        ),
        1
    );

    let echo_labels = member(
        &fixture,
        MemberKind::Method,
        "echoLabels",
        "([Ljava/lang/String;)[Ljava/lang/String;",
    );
    assert_eq!(
        ref_value(
            call_member(
                &fixture,
                fixture.method_class,
                echo_labels,
                "getReturnType",
                METHOD_GET_RETURN_TYPE,
                Vec::new(),
            )
            .unwrap()
        ),
        string_array
    );
    assert_eq!(
        array_elements(
            &fixture.shared,
            ref_value(
                call_member(
                    &fixture,
                    fixture.method_class,
                    echo_labels,
                    "getParameterTypes",
                    PARAMETER_TYPES,
                    Vec::new(),
                )
                .unwrap()
            )
        ),
        [Value::Ref(string_array)]
    );

    let count = member(&fixture, MemberKind::Method, "count", "([I)I");
    assert_eq!(
        ref_value(
            call_member(
                &fixture,
                fixture.method_class,
                count,
                "getReturnType",
                METHOD_GET_RETURN_TYPE,
                Vec::new(),
            )
            .unwrap()
        ),
        fixture.shared.primitive_class(PrimitiveKind::Int),
        "an int return type is still the primitive class object"
    );
    assert_eq!(
        array_elements(
            &fixture.shared,
            ref_value(
                call_member(
                    &fixture,
                    fixture.method_class,
                    count,
                    "getParameterTypes",
                    PARAMETER_TYPES,
                    Vec::new(),
                )
                .unwrap()
            )
        ),
        [Value::Ref(int_array)]
    );

    // Constructors.
    let array_constructor = member(&fixture, MemberKind::Constructor, "<init>", "([I)V");
    assert_eq!(
        array_elements(
            &fixture.shared,
            ref_value(
                call_member(
                    &fixture,
                    fixture.constructor_class,
                    array_constructor,
                    "getParameterTypes",
                    PARAMETER_TYPES,
                    Vec::new(),
                )
                .unwrap()
            )
        ),
        [Value::Ref(int_array)],
        "Constructor.getParameterTypes of ([I)V"
    );
    assert_eq!(
        int_value(
            call_member(
                &fixture,
                fixture.constructor_class,
                array_constructor,
                "getParameterCount",
                MEMBER_GET_PARAMETER_COUNT,
                Vec::new(),
            )
            .unwrap()
        ),
        1
    );

    let both_constructor = member(
        &fixture,
        MemberKind::Constructor,
        "<init>",
        "([I[Ljava/lang/String;)V",
    );
    assert_eq!(
        array_elements(
            &fixture.shared,
            ref_value(
                call_member(
                    &fixture,
                    fixture.constructor_class,
                    both_constructor,
                    "getParameterTypes",
                    PARAMETER_TYPES,
                    Vec::new(),
                )
                .unwrap()
            )
        ),
        [Value::Ref(int_array), Value::Ref(string_array)],
        "array parameters keep their order"
    );

    // The bytecode path builds the same member types.
    assert_eq!(
        string_value(
            &fixture.shared,
            call_static(
                &fixture,
                "fieldTypeName",
                "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/String;",
                vec![
                    Value::Ref(fixture.bean_object),
                    name_value(&fixture, "values"),
                ],
            )
            .unwrap()
        ),
        "[I"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            call_static(
                &fixture,
                "methodReturnTypeName",
                "(Ljava/lang/Class;Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/String;",
                vec![
                    Value::Ref(fixture.bean_object),
                    name_value(&fixture, "echo"),
                    params(&fixture, &[int_array]),
                ],
            )
            .unwrap()
        ),
        "[I"
    );
    assert_eq!(
        array_elements(
            &fixture.shared,
            ref_value(
                call_static(
                    &fixture,
                    "methodParameterTypes",
                    "(Ljava/lang/Class;Ljava/lang/String;[Ljava/lang/Class;)[Ljava/lang/Class;",
                    vec![
                        Value::Ref(fixture.bean_object),
                        name_value(&fixture, "echoLabels"),
                        params(&fixture, &[string_array]),
                    ],
                )
                .unwrap()
            )
        ),
        [Value::Ref(string_array)]
    );
    assert_eq!(
        array_elements(
            &fixture.shared,
            ref_value(
                call_static(
                    &fixture,
                    "constructorParameterTypes",
                    "(Ljava/lang/Class;[Ljava/lang/Class;)[Ljava/lang/Class;",
                    vec![
                        Value::Ref(fixture.bean_object),
                        params(&fixture, &[int_array, string_array]),
                    ],
                )
                .unwrap()
            )
        ),
        [Value::Ref(int_array), Value::Ref(string_array)]
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            call_static(
                &fixture,
                "parameterTypeName",
                "(Ljava/lang/Class;Ljava/lang/String;[Ljava/lang/Class;I)Ljava/lang/String;",
                vec![
                    Value::Ref(fixture.bean_object),
                    name_value(&fixture, "echo"),
                    params(&fixture, &[int_array]),
                    Value::Int(0),
                ],
            )
            .unwrap()
        ),
        "[I"
    );
}

#[test]
fn named_lookup_matches_array_parameter_classes() {
    let fixture = fixture();
    let int_array = array_class(&fixture, "[I");
    let string_array = array_class(&fixture, "[Ljava.lang.String;");
    let long_array = array_class(&fixture, "[J");
    let int_primitive = fixture.shared.primitive_class(PrimitiveKind::Int);

    let echo =
        ref_value(declared_method(&fixture, fixture.bean_object, "echo", &[int_array]).unwrap());
    let echo_member = member_of(&fixture, echo);
    assert_eq!(echo_member.kind, MemberKind::Method);
    assert_eq!(echo_member.name, "echo");
    assert_eq!(
        echo_member.descriptor, "([I)[I",
        "an int[].class parameter builds the [I descriptor"
    );
    assert_eq!(
        echo,
        member(&fixture, MemberKind::Method, "echo", "([I)[I"),
        "the lookup returns the cached member object"
    );
    assert_eq!(
        declared_method(&fixture, fixture.bean_object, "echo", &[int_array]).unwrap(),
        Some(Value::Ref(echo)),
        "a second lookup is reference-identical"
    );

    // The bytecode client builds the same query with `Class.forName("[I")`.
    let via_bytecode = ref_value(
        call_static(
            &fixture,
            "declaredMethod",
            "(Ljava/lang/Class;Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;",
            vec![
                Value::Ref(fixture.bean_object),
                name_value(&fixture, "echo"),
                params(&fixture, &[int_array]),
            ],
        )
        .unwrap(),
    );
    assert_eq!(via_bytecode, echo);

    let echo_labels = ref_value(
        declared_method(&fixture, fixture.bean_object, "echoLabels", &[string_array]).unwrap(),
    );
    assert_eq!(
        member_of(&fixture, echo_labels).descriptor,
        "([Ljava/lang/String;)[Ljava/lang/String;"
    );
    let count =
        ref_value(declared_method(&fixture, fixture.bean_object, "count", &[int_array]).unwrap());
    assert_eq!(member_of(&fixture, count).descriptor, "([I)I");

    // Mismatches are the documented null miss.
    assert!(is_null(declared_method(
        &fixture,
        fixture.bean_object,
        "echo",
        &[string_array]
    )));
    assert!(is_null(declared_method(
        &fixture,
        fixture.bean_object,
        "echo",
        &[long_array]
    )));
    assert!(is_null(declared_method(
        &fixture,
        fixture.bean_object,
        "echo",
        &[int_primitive]
    )));
    assert!(is_null(declared_method(
        &fixture,
        fixture.bean_object,
        "echo",
        &[]
    )));
    assert!(is_null(declared_method(
        &fixture,
        fixture.bean_object,
        "echo",
        &[int_array, int_array]
    )));
    assert!(is_null(declared_method(
        &fixture,
        fixture.bean_object,
        "count",
        &[string_array]
    )));

    // `getMethod` accepts the same array `Class[]` and returns the same cached object.
    assert_eq!(
        ref_value(
            call_class(
                &fixture,
                fixture.bean_object,
                "getMethod",
                DECLARED_METHOD,
                vec![name_value(&fixture, "echo"), params(&fixture, &[int_array]),],
            )
            .unwrap()
        ),
        echo
    );

    // Constructors.
    let array_constructor =
        ref_value(declared_constructor(&fixture, fixture.bean_object, &[int_array]).unwrap());
    assert_eq!(member_of(&fixture, array_constructor).descriptor, "([I)V");
    let both_constructor = ref_value(
        declared_constructor(&fixture, fixture.bean_object, &[int_array, string_array]).unwrap(),
    );
    assert_eq!(
        member_of(&fixture, both_constructor).descriptor,
        "([I[Ljava/lang/String;)V"
    );
    assert!(is_null(declared_constructor(
        &fixture,
        fixture.bean_object,
        &[string_array]
    )));
    assert!(is_null(declared_constructor(
        &fixture,
        fixture.bean_object,
        &[long_array]
    )));
    assert!(is_null(declared_constructor(
        &fixture,
        fixture.bean_object,
        &[int_array, int_array]
    )));

    // An array class declares no members, exactly as on the JDK.
    let fields = ref_value(
        call_class(
            &fixture,
            int_array,
            "getDeclaredFields",
            DECLARED_FIELDS,
            Vec::new(),
        )
        .unwrap(),
    );
    assert!(array_elements(&fixture.shared, fields).is_empty());
    assert!(is_null(declared_method(
        &fixture,
        int_array,
        "echo",
        &[int_array]
    )));
    assert!(is_null(declared_constructor(
        &fixture,
        int_array,
        &[int_array]
    )));
    assert!(is_null(call_class(
        &fixture,
        int_array,
        "getDeclaredField",
        DECLARED_FIELD,
        vec![name_value(&fixture, "length")],
    )));
}

#[test]
fn array_class_missing_and_mismatch_behavior_is_typed() {
    let fixture = fixture();

    // A malformed array descriptor is a typed native error, not a guessed class.
    assert_native_error(
        call_static(
            &fixture,
            "forNameClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            vec![name_value(&fixture, "[Q")],
        ),
        "malformed array descriptor",
    );
    assert_native_error(
        call_static(
            &fixture,
            "forNameClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            vec![name_value(&fixture, "[V")],
        ),
        "malformed array descriptor",
    );
    assert_native_error(
        call_static(
            &fixture,
            "forNameClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            vec![name_value(&fixture, "[L;")],
        ),
        "malformed array descriptor",
    );

    // A well-formed component that the bootstrap loader does not define is a loader
    // error, exactly as a missing named class is.
    assert_loader_error(
        call_static(
            &fixture,
            "forNameClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            vec![name_value(&fixture, "[Ljava.lang.NoSuchClass;")],
        ),
        "java/lang/NoSuchClass",
    );

    // An array class is not the `Class` of the array's component, and the cache keeps
    // the two apart.
    let int_array = array_class(&fixture, "[I");
    let string_array = array_class(&fixture, "[Ljava.lang.String;");
    let int_primitive = fixture.shared.primitive_class(PrimitiveKind::Int);
    assert_ne!(int_array, int_primitive);
    assert_ne!(int_array, string_array);

    // A named class still reports non-array and non-primitive.
    assert!(!bool_value(
        call_static(
            &fixture,
            "isArray",
            "(Ljava/lang/Class;)Z",
            vec![Value::Ref(fixture.string_object)],
        )
        .unwrap()
    ));
    assert!(!bool_value(
        call_static(
            &fixture,
            "isPrimitive",
            "(Ljava/lang/Class;)Z",
            vec![Value::Ref(fixture.string_object)],
        )
        .unwrap()
    ));
    assert!(
        ref_value(
            call_static(
                &fixture,
                "componentType",
                "(Ljava/lang/Class;)Ljava/lang/Class;",
                vec![Value::Ref(fixture.string_object)],
            )
            .unwrap()
        )
        .is_null()
    );
}

#[test]
fn array_class_cache_is_stable_and_rooted() {
    let fixture = fixture();

    let int_array = array_class(&fixture, "[I");
    let int_grid = array_class(&fixture, "[[I");
    assert_ne!(int_array, int_grid);
    assert_eq!(
        fixture
            .shared
            .heap()
            .array_class(int_grid)
            .unwrap()
            .component,
        ClassRef::Array(Box::new(rava_vm::ArrayClass::new(ClassRef::Primitive(
            PrimitiveKind::Int
        ))))
    );

    // The nested array's component is the cached `int[]` object, not a second object.
    assert_eq!(
        ref_value(
            call_static(
                &fixture,
                "componentType",
                "(Ljava/lang/Class;)Ljava/lang/Class;",
                vec![Value::Ref(int_grid)],
            )
            .unwrap()
        ),
        int_array,
        "getComponentType([[I) is the cached int[]"
    );
    assert_eq!(
        fixture
            .shared
            .array_class(ClassRef::Primitive(PrimitiveKind::Int))
            .unwrap(),
        int_array,
        "the cache key is the component"
    );

    let int_primitive = fixture.shared.primitive_class(PrimitiveKind::Int);
    let string_array = array_class(&fixture, "[Ljava.lang.String;");
    let roots = fixture.shared.roots(&[]);
    assert!(roots.contains(&int_array));
    assert!(roots.contains(&int_grid));
    assert!(
        roots.contains(&int_primitive),
        "the component primitive is a root"
    );
    assert!(roots.contains(&fixture.object_object));

    // A second array class with a different component is a different root and object.
    assert!(roots.contains(&string_array));
    assert_ne!(int_array, string_array);
}
