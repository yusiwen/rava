//! M2.14 acceptance: named member lookup on `Class`.
//!
//! `LookupBean` is an ordinary class with a private `int value` and `String label`, three
//! constructors (`()V`, `(I)V`, `(Ljava/lang/String;)V`), int/reference accessors and a
//! static `sum(II)I`; `LookupChild` extends it, hides `value` with its own field and
//! implements `LookupContract` without declaring the interface method. `LookupProbe` is a
//! bytecode client, so the virtual-dispatch path from a `Class` receiver through the native
//! registry is exercised as well as the direct native calls.
//!
//! Lookup misses are `null` in this slice (the JDK throws `NoSuchFieldException` or
//! `NoSuchMethodException`; exception synthesis does not exist yet), and a primitive or
//! array parameter class is a typed error because those class objects are not
//! representable. Both outcomes are asserted exactly.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_instance_method, run_static_method};
use rava_vm::{
    ArrayKind, ClassId, LoaderId, MemberKind, ObjRef, ReflectMember, Shared, ThreadState, Value,
    VmError, instance, object, reflection,
};

const ACC_PRIVATE: u16 = 0x0002;
const ACC_PUBLIC: u16 = 0x0001;
const ACC_PUBLIC_STATIC: u16 = 0x0009;
const ACC_PUBLIC_ABSTRACT: u16 = 0x0401;
const ACC_PUBLIC_INTERFACE: u16 = 0x0601;

const DECLARED_FIELD: &str = "(Ljava/lang/String;)Ljava/lang/reflect/Field;";
const INHERITED_FIELD: &str = "(Ljava/lang/String;)Ljava/lang/reflect/Field;";
const DECLARED_METHOD: &str = "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;";
const INHERITED_METHOD: &str = "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;";
const DECLARED_CONSTRUCTOR: &str = "([Ljava/lang/Class;)Ljava/lang/reflect/Constructor;";
const INHERITED_CONSTRUCTOR: &str = "([Ljava/lang/Class;)Ljava/lang/reflect/Constructor;";

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn method(
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

/// `LookupBean`: the base fixture. Its constructors make the declared-constructor lookup
/// meaningful, and `value`/`label` cover an `int` and a reference field.
fn bean_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("LookupBean", "java/lang/Object", 61);
    builder.field(ACC_PRIVATE, "value", "I");
    builder.field(ACC_PRIVATE, "label", "Ljava/lang/String;");

    let object_init = builder.method_ref("java/lang/Object", "<init>", "()V");
    let value_ref = builder.field_ref("LookupBean", "value", "I");
    let label_ref = builder.field_ref("LookupBean", "label", "Ljava/lang/String;");

    // LookupBean() { super(); }
    let mut init = Assembler::new();
    init.aload(0);
    init.invokespecial(object_init);
    init.return_();
    method(&mut builder, ACC_PUBLIC, "<init>", "()V", 1, 1, init);

    // LookupBean(int value) { super(); this.value = value; }
    let mut init_int = Assembler::new();
    init_int.aload(0);
    init_int.invokespecial(object_init);
    init_int.aload(0);
    init_int.iload(1);
    init_int.putfield(value_ref);
    init_int.return_();
    method(&mut builder, ACC_PUBLIC, "<init>", "(I)V", 2, 2, init_int);

    // LookupBean(String label) { super(); this.label = label; }
    let mut init_string = Assembler::new();
    init_string.aload(0);
    init_string.invokespecial(object_init);
    init_string.aload(0);
    init_string.aload(1);
    init_string.putfield(label_ref);
    init_string.return_();
    method(
        &mut builder,
        ACC_PUBLIC,
        "<init>",
        "(Ljava/lang/String;)V",
        2,
        2,
        init_string,
    );

    // int getValue() { return this.value; }
    let mut get_value = Assembler::new();
    get_value.aload(0);
    get_value.getfield(value_ref);
    get_value.ireturn();
    method(&mut builder, ACC_PUBLIC, "getValue", "()I", 1, 1, get_value);

    // void setValue(int value) { this.value = value; }
    let mut set_value = Assembler::new();
    set_value.aload(0);
    set_value.iload(1);
    set_value.putfield(value_ref);
    set_value.return_();
    method(
        &mut builder,
        ACC_PUBLIC,
        "setValue",
        "(I)V",
        2,
        2,
        set_value,
    );

    // void setLabel(String label) { this.label = label; }
    let mut set_label = Assembler::new();
    set_label.aload(0);
    set_label.aload(1);
    set_label.putfield(label_ref);
    set_label.return_();
    method(
        &mut builder,
        ACC_PUBLIC,
        "setLabel",
        "(Ljava/lang/String;)V",
        2,
        2,
        set_label,
    );

    // static int sum(int left, int right) { return left + right; }
    let mut sum = Assembler::new();
    sum.iload(0);
    sum.iload(1);
    sum.iadd();
    sum.ireturn();
    method(&mut builder, ACC_PUBLIC_STATIC, "sum", "(II)I", 2, 2, sum);

    builder.build()
}

/// `LookupChild`: no constructor of its own (constructors are not inherited), hides the
/// parent's `value` field, and implements `LookupContract` without declaring `contract`.
fn child_bytes() -> Vec<u8> {
    let mut builder =
        ClassBuilder::new("LookupChild", "LookupBean", 61).with_access(ACC_PUBLIC_ABSTRACT);
    builder.field(ACC_PRIVATE, "value", "I");
    builder.interface("LookupContract");
    builder.build()
}

/// `LookupContract`: an interface whose method is only declared here.
fn contract_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("LookupContract", "java/lang/Object", 61)
        .with_access(ACC_PUBLIC_INTERFACE);
    builder.method(
        ACC_PUBLIC_ABSTRACT,
        "contract",
        "()Ljava/lang/String;",
        None,
    );
    builder.build()
}

/// `LookupProbe`: a bytecode client for the lookup natives, so their dispatch from a
/// `Class` receiver is exercised, not only the Rust-side native call.
fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("LookupProbe", "java/lang/Object", 61);
    let class_class = builder.class("java/lang/Class");
    let get_declared_field =
        builder.method_ref("java/lang/Class", "getDeclaredField", DECLARED_FIELD);
    let get_field = builder.method_ref("java/lang/Class", "getField", INHERITED_FIELD);
    let get_declared_method =
        builder.method_ref("java/lang/Class", "getDeclaredMethod", DECLARED_METHOD);
    let get_method = builder.method_ref("java/lang/Class", "getMethod", INHERITED_METHOD);
    let get_declared_constructor = builder.method_ref(
        "java/lang/Class",
        "getDeclaredConstructor",
        DECLARED_CONSTRUCTOR,
    );
    let value_name = builder.string("value");
    let get_value_name = builder.string("getValue");

    // static Field declaredValue(Class c) { return c.getDeclaredField("value"); }
    let mut declared_value = Assembler::new();
    declared_value.aload(0);
    declared_value.ldc(value_name);
    declared_value.invokevirtual(get_declared_field);
    declared_value.areturn();
    method(
        &mut builder,
        ACC_PUBLIC_STATIC,
        "declaredValue",
        "(Ljava/lang/Class;)Ljava/lang/reflect/Field;",
        2,
        1,
        declared_value,
    );

    // static Field inheritedValue(Class c) { return c.getField("value"); }
    let mut inherited_value = Assembler::new();
    inherited_value.aload(0);
    inherited_value.ldc(value_name);
    inherited_value.invokevirtual(get_field);
    inherited_value.areturn();
    method(
        &mut builder,
        ACC_PUBLIC_STATIC,
        "inheritedValue",
        "(Ljava/lang/Class;)Ljava/lang/reflect/Field;",
        2,
        1,
        inherited_value,
    );

    // static Method declaredGetValue(Class c) {
    //     return c.getDeclaredMethod("getValue", new Class[0]);
    // }
    let mut declared_get_value = Assembler::new();
    declared_get_value.aload(0);
    declared_get_value.ldc(get_value_name);
    declared_get_value.iconst(0);
    declared_get_value.anewarray(class_class);
    declared_get_value.invokevirtual(get_declared_method);
    declared_get_value.areturn();
    method(
        &mut builder,
        ACC_PUBLIC_STATIC,
        "declaredGetValue",
        "(Ljava/lang/Class;)Ljava/lang/reflect/Method;",
        3,
        1,
        declared_get_value,
    );

    // static Method inheritedGetValue(Class c) {
    //     return c.getMethod("getValue", new Class[0]);
    // }
    let mut inherited_get_value = Assembler::new();
    inherited_get_value.aload(0);
    inherited_get_value.ldc(get_value_name);
    inherited_get_value.iconst(0);
    inherited_get_value.anewarray(class_class);
    inherited_get_value.invokevirtual(get_method);
    inherited_get_value.areturn();
    method(
        &mut builder,
        ACC_PUBLIC_STATIC,
        "inheritedGetValue",
        "(Ljava/lang/Class;)Ljava/lang/reflect/Method;",
        3,
        1,
        inherited_get_value,
    );

    // static Constructor defaultConstructor(Class c) {
    //     return c.getDeclaredConstructor(new Class[0]);
    // }
    let mut default_constructor = Assembler::new();
    default_constructor.aload(0);
    default_constructor.iconst(0);
    default_constructor.anewarray(class_class);
    default_constructor.invokevirtual(get_declared_constructor);
    default_constructor.areturn();
    method(
        &mut builder,
        ACC_PUBLIC_STATIC,
        "defaultConstructor",
        "(Ljava/lang/Class;)Ljava/lang/reflect/Constructor;",
        2,
        1,
        default_constructor,
    );

    builder.build()
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    bean: ClassId,
    child: ClassId,
    contract: ClassId,
    probe: ClassId,
    bean_object: ObjRef,
    child_object: ObjRef,
    contract_object: ObjRef,
    string_object: ObjRef,
    object_object: ObjRef,
    class_class: ClassId,
    object_class: ClassId,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    instance::register_natives(&shared);
    object::register_natives(&shared);
    reflection::register_natives(&shared);

    let bean = shared
        .define_class(LoaderId::BOOTSTRAP, "LookupBean", &bean_bytes())
        .expect("LookupBean defines");
    let child = shared
        .define_class(LoaderId::BOOTSTRAP, "LookupChild", &child_bytes())
        .expect("LookupChild defines");
    let contract = shared
        .define_class(LoaderId::BOOTSTRAP, "LookupContract", &contract_bytes())
        .expect("LookupContract defines");
    let probe = shared
        .define_class(LoaderId::BOOTSTRAP, "LookupProbe", &probe_bytes())
        .expect("LookupProbe defines");

    let load = |name: &str| {
        shared
            .load_library_class(name)
            .unwrap_or_else(|error| panic!("{name} loads: {error}"))
    };
    let class_class = load("java/lang/Class");
    let string_class = load("java/lang/String");
    let object_class = load("java/lang/Object");

    let bean_object = shared.class_object(bean).expect("LookupBean class object");
    let child_object = shared
        .class_object(child)
        .expect("LookupChild class object");
    let contract_object = shared
        .class_object(contract)
        .expect("LookupContract class object");
    let string_object = shared
        .class_object(string_class)
        .expect("String class object");
    let object_object = shared
        .class_object(object_class)
        .expect("Object class object");
    let thread = shared.thread_state(1);
    Fixture {
        shared,
        thread,
        bean,
        child,
        contract,
        probe,
        bean_object,
        child_object,
        contract_object,
        string_object,
        object_object,
        class_class,
        object_class,
    }
}

fn reference_array(shared: &Shared, values: &[ObjRef]) -> ObjRef {
    let mut heap = shared.heap();
    let array = heap.allocate_array(ArrayKind::Reference, values.len());
    {
        let array = heap.array_mut(array).expect("a reference array");
        for (slot, value) in array.elements.iter_mut().zip(values) {
            *slot = Value::Ref(*value);
        }
    }
    array
}

fn array_elements(shared: &Shared, reference: ObjRef) -> Vec<Value> {
    let heap = shared.heap();
    heap.array(reference)
        .expect("a heap array")
        .elements
        .clone()
}

/// Call one lookup native directly, with the `java.lang.Class` receiver class.
fn lookup(
    fixture: &Fixture,
    name: &str,
    descriptor: &str,
    receiver: ObjRef,
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

fn name_value(fixture: &Fixture, name: &str) -> Value {
    Value::Ref(fixture.shared.intern_string(name))
}

fn params(fixture: &Fixture, classes: &[ObjRef]) -> Value {
    Value::Ref(reference_array(&fixture.shared, classes))
}

fn declared_field(
    fixture: &Fixture,
    class: ObjRef,
    name: &str,
) -> Result<Option<Value>, InterpError> {
    lookup(
        fixture,
        "getDeclaredField",
        DECLARED_FIELD,
        class,
        vec![name_value(fixture, name)],
    )
}

fn inherited_field(
    fixture: &Fixture,
    class: ObjRef,
    name: &str,
) -> Result<Option<Value>, InterpError> {
    lookup(
        fixture,
        "getField",
        INHERITED_FIELD,
        class,
        vec![name_value(fixture, name)],
    )
}

fn declared_method(
    fixture: &Fixture,
    class: ObjRef,
    name: &str,
    parameters: &[ObjRef],
) -> Result<Option<Value>, InterpError> {
    lookup(
        fixture,
        "getDeclaredMethod",
        DECLARED_METHOD,
        class,
        vec![name_value(fixture, name), params(fixture, parameters)],
    )
}

fn inherited_method(
    fixture: &Fixture,
    class: ObjRef,
    name: &str,
    parameters: &[ObjRef],
) -> Result<Option<Value>, InterpError> {
    lookup(
        fixture,
        "getMethod",
        INHERITED_METHOD,
        class,
        vec![name_value(fixture, name), params(fixture, parameters)],
    )
}

fn declared_constructor(
    fixture: &Fixture,
    class: ObjRef,
    parameters: &[ObjRef],
) -> Result<Option<Value>, InterpError> {
    lookup(
        fixture,
        "getDeclaredConstructor",
        DECLARED_CONSTRUCTOR,
        class,
        vec![params(fixture, parameters)],
    )
}

fn inherited_constructor(
    fixture: &Fixture,
    class: ObjRef,
    parameters: &[ObjRef],
) -> Result<Option<Value>, InterpError> {
    lookup(
        fixture,
        "getConstructor",
        INHERITED_CONSTRUCTOR,
        class,
        vec![params(fixture, parameters)],
    )
}

/// The reference a successful lookup returned, failing loudly on `null` or an error.
fn member_ref(result: Result<Option<Value>, InterpError>) -> ObjRef {
    match result.unwrap_or_else(|error| panic!("lookup failed: {error}")) {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        other => panic!("expected a member reference, found {other:?}"),
    }
}

/// The identity behind a returned member object.
fn member_view(fixture: &Fixture, reference: ObjRef) -> ReflectMember {
    fixture
        .shared
        .heap()
        .reflect(reference)
        .expect("a reflected member")
        .clone()
}

fn assert_member(
    view: &ReflectMember,
    kind: MemberKind,
    class: ClassId,
    name: &str,
    descriptor: &str,
) {
    assert_eq!(view.kind, kind, "member kind for {name}{descriptor}");
    assert_eq!(view.class, class, "declaring class for {name}{descriptor}");
    assert_eq!(view.name, name, "member name");
    assert_eq!(view.descriptor, descriptor, "member descriptor");
}

fn is_null(result: Result<Option<Value>, InterpError>) -> bool {
    match result.unwrap_or_else(|error| panic!("lookup failed: {error}")) {
        Some(Value::Ref(reference)) => reference.is_null(),
        other => panic!("expected a reference result, found {other:?}"),
    }
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

#[test]
fn declared_and_inherited_field_lookup() {
    let fixture = fixture();

    let value = member_ref(declared_field(&fixture, fixture.bean_object, "value"));
    assert_member(
        &member_view(&fixture, value),
        MemberKind::Field,
        fixture.bean,
        "value",
        "I",
    );
    // The lookup returns the same object the cache and the array query hold.
    let cached = fixture
        .shared
        .reflect_member(fixture.bean, MemberKind::Field, "value", "I")
        .expect("the cached member");
    assert_eq!(value, cached);
    assert_eq!(
        member_ref(declared_field(&fixture, fixture.bean_object, "value")),
        value,
        "a second lookup returns the cached object"
    );

    let fields = member_ref(lookup(
        &fixture,
        "getDeclaredFields",
        "()[Ljava/lang/reflect/Field;",
        fixture.bean_object,
        Vec::new(),
    ));
    let elements = array_elements(&fixture.shared, fields);
    assert!(
        elements.contains(&Value::Ref(value)),
        "getDeclaredFields contains the looked-up object"
    );

    let label = member_ref(declared_field(&fixture, fixture.bean_object, "label"));
    assert_member(
        &member_view(&fixture, label),
        MemberKind::Field,
        fixture.bean,
        "label",
        "Ljava/lang/String;",
    );

    // `getField` finds the parent field on a subclass, but the subclass hides `value`, so
    // the class-first order reports the child's declaration.
    assert_eq!(
        member_ref(inherited_field(&fixture, fixture.bean_object, "value")),
        value,
        "getField on the declaring class returns the same cached object"
    );
    let child_value = member_ref(declared_field(&fixture, fixture.child_object, "value"));
    assert_member(
        &member_view(&fixture, child_value),
        MemberKind::Field,
        fixture.child,
        "value",
        "I",
    );
    assert_eq!(
        member_ref(inherited_field(&fixture, fixture.child_object, "value")),
        child_value,
        "getField searches the class before its superclass"
    );
    let inherited_label = member_ref(inherited_field(&fixture, fixture.child_object, "label"));
    assert_member(
        &member_view(&fixture, inherited_label),
        MemberKind::Field,
        fixture.bean,
        "label",
        "Ljava/lang/String;",
    );

    // A miss is null in this slice, as documented.
    assert!(is_null(declared_field(
        &fixture,
        fixture.bean_object,
        "missing"
    )));
    assert!(is_null(inherited_field(
        &fixture,
        fixture.child_object,
        "missing"
    )));
}

#[test]
fn declared_and_inherited_method_lookup() {
    let fixture = fixture();

    let get_value = member_ref(declared_method(
        &fixture,
        fixture.bean_object,
        "getValue",
        &[],
    ));
    assert_member(
        &member_view(&fixture, get_value),
        MemberKind::Method,
        fixture.bean,
        "getValue",
        "()I",
    );
    assert_eq!(
        get_value,
        fixture
            .shared
            .reflect_member(fixture.bean, MemberKind::Method, "getValue", "()I")
            .expect("the cached member")
    );
    assert_eq!(
        member_ref(inherited_method(
            &fixture,
            fixture.bean_object,
            "getValue",
            &[]
        )),
        get_value
    );
    assert_eq!(
        member_ref(inherited_method(
            &fixture,
            fixture.child_object,
            "getValue",
            &[]
        )),
        get_value,
        "getMethod reports the inherited member, declared on the superclass"
    );
    assert!(
        is_null(declared_method(
            &fixture,
            fixture.child_object,
            "getValue",
            &[]
        )),
        "getDeclaredMethod does not see an inherited method"
    );

    // A reference parameter is matched by its class object, so an overload is selected by
    // name plus parameter types, not by name alone.
    let string_type = fixture.string_object;
    let set_label = member_ref(declared_method(
        &fixture,
        fixture.bean_object,
        "setLabel",
        &[string_type],
    ));
    assert_member(
        &member_view(&fixture, set_label),
        MemberKind::Method,
        fixture.bean,
        "setLabel",
        "(Ljava/lang/String;)V",
    );
    assert!(is_null(declared_method(
        &fixture,
        fixture.bean_object,
        "setLabel",
        &[]
    )));
    assert!(is_null(declared_method(
        &fixture,
        fixture.bean_object,
        "setLabel",
        &[fixture.object_object]
    )));

    // An interface method that the class does not declare is still found by getMethod.
    let contract = member_ref(inherited_method(
        &fixture,
        fixture.child_object,
        "contract",
        &[],
    ));
    assert_member(
        &member_view(&fixture, contract),
        MemberKind::Method,
        fixture.contract,
        "contract",
        "()Ljava/lang/String;",
    );
    assert!(
        is_null(declared_method(
            &fixture,
            fixture.child_object,
            "contract",
            &[]
        )),
        "getDeclaredMethod does not see an interface method"
    );
    assert!(
        is_null(inherited_method(
            &fixture,
            fixture.bean_object,
            "contract",
            &[]
        )),
        "a class that does not implement the interface does not find its method"
    );

    // The superclass chain includes Object.
    let to_string = member_ref(inherited_method(
        &fixture,
        fixture.child_object,
        "toString",
        &[],
    ));
    assert_member(
        &member_view(&fixture, to_string),
        MemberKind::Method,
        fixture.object_class,
        "toString",
        "()Ljava/lang/String;",
    );

    // The return type is not part of a reflective method query.
    assert!(is_null(inherited_method(
        &fixture,
        fixture.bean_object,
        "getValue",
        &[fixture.string_object]
    )));
    assert!(is_null(declared_method(
        &fixture,
        fixture.bean_object,
        "missing",
        &[]
    )));
}

#[test]
fn declared_and_inherited_constructor_lookup() {
    let fixture = fixture();

    let default_constructor = member_ref(declared_constructor(&fixture, fixture.bean_object, &[]));
    assert_member(
        &member_view(&fixture, default_constructor),
        MemberKind::Constructor,
        fixture.bean,
        "<init>",
        "()V",
    );
    assert_eq!(
        default_constructor,
        fixture
            .shared
            .reflect_member(fixture.bean, MemberKind::Constructor, "<init>", "()V")
            .expect("the cached constructor")
    );
    assert_eq!(
        member_ref(inherited_constructor(&fixture, fixture.bean_object, &[])),
        default_constructor,
        "getConstructor on the declaring class returns the same cached object"
    );

    // A reference parameter selects the String constructor.
    let string_constructor = member_ref(declared_constructor(
        &fixture,
        fixture.bean_object,
        &[fixture.string_object],
    ));
    assert_member(
        &member_view(&fixture, string_constructor),
        MemberKind::Constructor,
        fixture.bean,
        "<init>",
        "(Ljava/lang/String;)V",
    );

    // Constructors are not inherited: the subclass declares none, so both queries miss
    // even though the superclass has a no-arg constructor.
    assert!(is_null(declared_constructor(
        &fixture,
        fixture.child_object,
        &[]
    )));
    assert!(is_null(inherited_constructor(
        &fixture,
        fixture.child_object,
        &[]
    )));
    assert!(is_null(declared_constructor(
        &fixture,
        fixture.contract_object,
        &[]
    )));

    // Parameter lists must match exactly.
    assert!(is_null(declared_constructor(
        &fixture,
        fixture.bean_object,
        &[fixture.object_object]
    )));
    assert!(is_null(declared_constructor(
        &fixture,
        fixture.bean_object,
        &[fixture.string_object, fixture.string_object]
    )));
}

#[test]
fn primitive_and_null_parameter_classes_are_typed_errors() {
    let fixture = fixture();

    // A primitive class object cannot be represented in this slice. A class whose binary
    // name is a primitive keyword is the stand-in that reaches the rejection branch; it is
    // never loaded as a real Java class and only exists to exercise the typed error.
    let primitive_stand_in = fixture
        .shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "int",
            &ClassBuilder::new("int", "java/lang/Object", 61).build(),
        )
        .expect("the primitive stand-in defines");
    let primitive_object = fixture
        .shared
        .class_object(primitive_stand_in)
        .expect("the primitive stand-in gets a class object");
    assert_native_error(
        declared_method(
            &fixture,
            fixture.bean_object,
            "sum",
            &[primitive_object, primitive_object],
        ),
        "primitive parameter classes",
    );

    // A null slot in the Class[] is a typed error, as is a null Class[] itself.
    let null_array = fixture
        .shared
        .heap()
        .allocate_array(ArrayKind::Reference, 1);
    assert_native_error(
        lookup(
            &fixture,
            "getDeclaredMethod",
            DECLARED_METHOD,
            fixture.bean_object,
            vec![name_value(&fixture, "getValue"), Value::Ref(null_array)],
        ),
        "null parameter class",
    );
    assert_native_error(
        lookup(
            &fixture,
            "getDeclaredMethod",
            DECLARED_METHOD,
            fixture.bean_object,
            vec![name_value(&fixture, "getValue"), Value::Ref(ObjRef::NULL)],
        ),
        "null Class[]",
    );

    // A parameter element that is not a Class object is a typed error, not a guess.
    let string_instance = fixture.shared.heap().allocate_string("not a class");
    let not_classes = reference_array(&fixture.shared, &[string_instance]);
    assert_native_error(
        lookup(
            &fixture,
            "getDeclaredMethod",
            DECLARED_METHOD,
            fixture.bean_object,
            vec![name_value(&fixture, "getValue"), Value::Ref(not_classes)],
        ),
        "not a Class object",
    );

    // A primitive array is not a Class[] either.
    let int_array = fixture.shared.heap().allocate_array(ArrayKind::Int, 0);
    assert_native_error(
        lookup(
            &fixture,
            "getDeclaredConstructor",
            DECLARED_CONSTRUCTOR,
            fixture.bean_object,
            vec![Value::Ref(int_array)],
        ),
        "primitive array",
    );
}

#[test]
fn the_lookup_natives_dispatch_from_bytecode() {
    let fixture = fixture();
    let call = |name: &str, descriptor: &str, receiver: ObjRef| {
        run_static_method(
            &fixture.shared,
            &fixture.thread,
            fixture.probe,
            name,
            descriptor,
            vec![Value::Ref(receiver)],
        )
    };

    let declared_value = member_ref(call(
        "declaredValue",
        "(Ljava/lang/Class;)Ljava/lang/reflect/Field;",
        fixture.bean_object,
    ));
    assert_eq!(
        declared_value,
        member_ref(declared_field(&fixture, fixture.bean_object, "value"))
    );

    let child_value = member_ref(call(
        "inheritedValue",
        "(Ljava/lang/Class;)Ljava/lang/reflect/Field;",
        fixture.child_object,
    ));
    assert_member(
        &member_view(&fixture, child_value),
        MemberKind::Field,
        fixture.child,
        "value",
        "I",
    );

    let get_value = member_ref(call(
        "declaredGetValue",
        "(Ljava/lang/Class;)Ljava/lang/reflect/Method;",
        fixture.bean_object,
    ));
    assert_eq!(
        get_value,
        member_ref(declared_method(
            &fixture,
            fixture.bean_object,
            "getValue",
            &[]
        ))
    );

    let inherited_get_value = member_ref(call(
        "inheritedGetValue",
        "(Ljava/lang/Class;)Ljava/lang/reflect/Method;",
        fixture.child_object,
    ));
    assert_eq!(
        inherited_get_value, get_value,
        "getMethod through the bytecode client reports the inherited cached member"
    );

    let default_constructor = member_ref(call(
        "defaultConstructor",
        "(Ljava/lang/Class;)Ljava/lang/reflect/Constructor;",
        fixture.bean_object,
    ));
    assert_eq!(
        default_constructor,
        member_ref(declared_constructor(&fixture, fixture.bean_object, &[]))
    );
}
