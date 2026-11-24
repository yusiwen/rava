//! M2.22 acceptance: the generic `Type` graph and generic reflection.
//!
//! `demo.Box`, `demo.StringBox`, `demo.Plain`, `demo.Nest` and `demo.GenericProbe` are
//! committed class files compiled with `javac --release 17`, so every `Signature` string is
//! exactly what a Java 17 compiler emits: a bounded class type parameter, a parameterized
//! superinterface, a `List<String>` field, `T[]`, `? extends Number`, `? super Integer`, a
//! member-class owner, a generic method and two generic constructors.
//!
//! The tests load the fixture through a child `DirectorySource` (no JDK at test time) and
//! drive the APIs through `demo.GenericProbe` bytecode, so `invokevirtual`/`invokeinterface`
//! dispatch, the Type object cache and the returned arrays are exercised rather than a
//! hand-built native call. The assertions cover the raw `Class` form, parameterized types
//! and their arguments, type-variable names and bounds, wildcard bounds, generic arrays,
//! field/method/constructor signatures, `getOwnerType`, the class identity `getClass`
//! reports for a Type object, and the typed errors a malformed signature produces.
//!
//! A Type object's class identity is the public `java.lang.reflect` interface it
//! implements (`ParameterizedType`, `TypeVariable`, ...), not the JDK's private
//! implementation class, exactly as a lambda's class identity is its functional interface
//! in this VM.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rava_classfile::ClassFile;
use rava_classfile::metadata::{
    SignatureError, TypeArgument, TypeSignature, parse_class_signature, parse_field_signature,
    parse_method_signature,
};
use rava_interp::{InterpError, run_instance_method, run_static_method};
use rava_vm::{
    ClassId, ClassRef, DirectorySource, HeapObject, LoaderId, MemberKind, ObjRef, Shared,
    ThreadState, TypeObject, Value, VmError, generic, instance, object, reflection,
};

const PROBE: &str = "demo/GenericProbe";
const TYPE: &str = "Ljava/lang/reflect/Type;";
const TYPE_ARRAY: &str = "[Ljava/lang/reflect/Type;";
const TYPE_VARIABLE: &str = "Ljava/lang/reflect/TypeVariable;";
const TYPE_VARIABLE_ARRAY: &str = "[Ljava/lang/reflect/TypeVariable;";
const CLASS: &str = "Ljava/lang/Class;";
const CLASS_ARRAY: &str = "[Ljava/lang/Class;";

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn fixture_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generics/classes")
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    loader: LoaderId,
    box_class: ClassId,
    box_object: ObjRef,
    string_box_object: ObjRef,
    plain_object: ObjRef,
    nest_object: ObjRef,
    probe: ClassId,
    class_class: ClassId,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    instance::register_natives(&shared);
    object::register_natives(&shared);
    reflection::register_natives(&shared);
    generic::register_natives(&shared);
    let loader = shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(fixture_classes())),
        )
        .expect("the fixture loader is added");
    let load = |name: &str| {
        shared
            .load_class(loader, name)
            .unwrap_or_else(|error| panic!("{name} loads: {error}"))
    };
    let box_class = load("demo/Box");
    let string_box = load("demo/StringBox");
    let plain = load("demo/Plain");
    let nest = load("demo/Nest");
    let probe = load(PROBE);
    let class_class = shared
        .load_library_class("java/lang/Class")
        .expect("java.lang.Class loads");
    Fixture {
        thread: shared.thread_state(1),
        box_class,
        box_object: shared.class_object(box_class).expect("Box class object"),
        string_box_object: shared
            .class_object(string_box)
            .expect("StringBox class object"),
        plain_object: shared.class_object(plain).expect("Plain class object"),
        nest_object: shared.class_object(nest).expect("Nest class object"),
        probe,
        class_class,
        shared,
        loader,
    }
}

fn call(
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

fn reference(value: Option<Value>) -> ObjRef {
    match value {
        Some(Value::Ref(reference)) => reference,
        other => panic!("expected a reference result, found {other:?}"),
    }
}

fn text(fixture: &Fixture, value: Option<Value>) -> String {
    let reference = reference(value);
    fixture
        .shared
        .heap()
        .string(reference)
        .expect("the result is a String")
        .to_owned()
}

fn class_array(fixture: &Fixture, values: &[ObjRef]) -> Value {
    let mut heap = fixture.shared.heap();
    let array = heap.allocate_array(ClassRef::Named(fixture.class_class), values.len());
    {
        let slots = heap.array_mut(array).expect("a heap array");
        for (slot, value) in slots.elements.iter_mut().zip(values.iter()) {
            *slot = Value::Ref(*value);
        }
    }
    Value::Ref(array)
}

fn elements(fixture: &Fixture, array: ObjRef) -> Vec<ObjRef> {
    fixture
        .shared
        .heap()
        .array(array)
        .expect("a heap array")
        .elements
        .iter()
        .map(|value| match value {
            Value::Ref(reference) => *reference,
            other => panic!("expected a reference element, found {other:?}"),
        })
        .collect()
}

fn class_object_of(fixture: &Fixture, name: &str) -> ObjRef {
    let class = fixture
        .shared
        .load_class(fixture.loader, name)
        .unwrap_or_else(|error| panic!("{name} loads: {error}"));
    fixture
        .shared
        .class_object(class)
        .expect("the class object allocates")
}

fn class_name(fixture: &Fixture, reference: ObjRef) -> String {
    let heap = fixture.shared.heap();
    match heap.get(reference).expect("a live reference") {
        HeapObject::Class(class) => fixture
            .shared
            .entry(*class)
            .expect("a loaded class")
            .name
            .replace('/', "."),
        HeapObject::PrimitiveClass(kind) => kind.name().to_owned(),
        HeapObject::ArrayClass(array) => fixture.shared.array_name(array).expect("an array name"),
        other => panic!("expected a class object, found {other:?}"),
    }
}

fn type_object(fixture: &Fixture, reference: ObjRef) -> TypeObject {
    fixture
        .shared
        .heap()
        .type_object(reference)
        .expect("a Type object")
        .clone()
}

fn parameterized(fixture: &Fixture, reference: ObjRef) -> rava_vm::ParameterizedType {
    match type_object(fixture, reference) {
        TypeObject::Parameterized(parameterized) => parameterized,
        other => panic!("expected a ParameterizedType, found {other:?}"),
    }
}

fn type_variable(fixture: &Fixture, reference: ObjRef) -> rava_vm::TypeVariableType {
    match type_object(fixture, reference) {
        TypeObject::TypeVariable(variable) => variable,
        other => panic!("expected a TypeVariable, found {other:?}"),
    }
}

fn wildcard(fixture: &Fixture, reference: ObjRef) -> rava_vm::WildcardType {
    match type_object(fixture, reference) {
        TypeObject::Wildcard(wildcard) => wildcard,
        other => panic!("expected a WildcardType, found {other:?}"),
    }
}

/// `superType(Class)`.
fn super_type(fixture: &Fixture, class: ObjRef) -> ObjRef {
    reference(
        call(
            fixture,
            "superType",
            &format!("({CLASS}){TYPE}"),
            vec![Value::Ref(class)],
        )
        .expect("getGenericSuperclass succeeds"),
    )
}

/// `firstInterface(Class)`.
fn first_interface(fixture: &Fixture, class: ObjRef) -> ObjRef {
    reference(
        call(
            fixture,
            "firstInterface",
            &format!("({CLASS}){TYPE}"),
            vec![Value::Ref(class)],
        )
        .expect("getGenericInterfaces succeeds"),
    )
}

/// `arguments(Type)`.
fn type_arguments(fixture: &Fixture, ty: ObjRef) -> Vec<ObjRef> {
    let array = reference(
        call(
            fixture,
            "arguments",
            &format!("({TYPE}){TYPE_ARRAY}"),
            vec![Value::Ref(ty)],
        )
        .expect("getActualTypeArguments succeeds"),
    );
    elements(fixture, array)
}

/// `fieldType(Class, String)`.
fn field_type(fixture: &Fixture, class: ObjRef, name: &str) -> ObjRef {
    reference(
        call(
            fixture,
            "fieldType",
            &format!("({CLASS}Ljava/lang/String;){TYPE}"),
            vec![
                Value::Ref(class),
                Value::Ref(fixture.shared.intern_string(name)),
            ],
        )
        .expect("getGenericType succeeds"),
    )
}

fn method_reference(fixture: &Fixture, name: &str, descriptor: &str) -> ObjRef {
    fixture
        .shared
        .reflect_member(fixture.box_class, MemberKind::Method, name, descriptor)
        .expect("the method object allocates")
}

fn constructor_reference(fixture: &Fixture, descriptor: &str) -> ObjRef {
    fixture
        .shared
        .reflect_member(
            fixture.box_class,
            MemberKind::Constructor,
            "<init>",
            descriptor,
        )
        .expect("the constructor object allocates")
}

/// `returnType(Class, String, Class...)`.
fn return_type(fixture: &Fixture, class: ObjRef, name: &str, parameters: &[ObjRef]) -> ObjRef {
    reference(
        call(
            fixture,
            "returnType",
            &format!("({CLASS}Ljava/lang/String;{CLASS_ARRAY}){TYPE}"),
            vec![
                Value::Ref(class),
                Value::Ref(fixture.shared.intern_string(name)),
                class_array(fixture, parameters),
            ],
        )
        .expect("getGenericReturnType succeeds"),
    )
}

/// `parameterTypes(Class, String, Class...)`.
fn parameter_types(
    fixture: &Fixture,
    class: ObjRef,
    name: &str,
    parameters: &[ObjRef],
) -> Vec<ObjRef> {
    let array = reference(
        call(
            fixture,
            "parameterTypes",
            &format!("({CLASS}Ljava/lang/String;{CLASS_ARRAY}){TYPE_ARRAY}"),
            vec![
                Value::Ref(class),
                Value::Ref(fixture.shared.intern_string(name)),
                class_array(fixture, parameters),
            ],
        )
        .expect("getGenericParameterTypes succeeds"),
    );
    elements(fixture, array)
}

/// `methodParameters(Class, String, Class...)`.
fn method_parameters(
    fixture: &Fixture,
    class: ObjRef,
    name: &str,
    parameters: &[ObjRef],
) -> Vec<ObjRef> {
    let array = reference(
        call(
            fixture,
            "methodParameters",
            &format!("({CLASS}Ljava/lang/String;{CLASS_ARRAY}){TYPE_VARIABLE_ARRAY}"),
            vec![
                Value::Ref(class),
                Value::Ref(fixture.shared.intern_string(name)),
                class_array(fixture, parameters),
            ],
        )
        .expect("getTypeParameters succeeds"),
    );
    elements(fixture, array)
}

/// `constructorParameterTypes(Class, Class...)`.
fn constructor_parameter_types(
    fixture: &Fixture,
    class: ObjRef,
    parameters: &[ObjRef],
) -> Vec<ObjRef> {
    let array = reference(
        call(
            fixture,
            "constructorParameterTypes",
            &format!("({CLASS}{CLASS_ARRAY}){TYPE_ARRAY}"),
            vec![Value::Ref(class), class_array(fixture, parameters)],
        )
        .expect("Constructor.getGenericParameterTypes succeeds"),
    );
    elements(fixture, array)
}

/// `constructorParameters(Class, Class...)`.
fn constructor_parameters(fixture: &Fixture, class: ObjRef, parameters: &[ObjRef]) -> Vec<ObjRef> {
    let array = reference(
        call(
            fixture,
            "constructorParameters",
            &format!("({CLASS}{CLASS_ARRAY}){TYPE_VARIABLE_ARRAY}"),
            vec![Value::Ref(class), class_array(fixture, parameters)],
        )
        .expect("Constructor.getTypeParameters succeeds"),
    );
    elements(fixture, array)
}

/// `bounds(TypeVariable)`.
fn bounds(fixture: &Fixture, variable: ObjRef) -> Vec<ObjRef> {
    let array = reference(
        call(
            fixture,
            "bounds",
            &format!("({TYPE_VARIABLE}){TYPE_ARRAY}"),
            vec![Value::Ref(variable)],
        )
        .expect("getBounds succeeds"),
    );
    elements(fixture, array)
}

/// `variableName(TypeVariable)`.
fn variable_name(fixture: &Fixture, variable: ObjRef) -> String {
    text(
        fixture,
        call(
            fixture,
            "variableName",
            &format!("({TYPE_VARIABLE})Ljava/lang/String;"),
            vec![Value::Ref(variable)],
        )
        .expect("getName succeeds"),
    )
}

/// `declaration(TypeVariable)`.
fn declaration(fixture: &Fixture, variable: ObjRef) -> ObjRef {
    reference(
        call(
            fixture,
            "declaration",
            &format!("({TYPE_VARIABLE})Ljava/lang/Object;"),
            vec![Value::Ref(variable)],
        )
        .expect("getGenericDeclaration succeeds"),
    )
}

fn type_name(fixture: &Fixture, ty: ObjRef) -> String {
    text(
        fixture,
        call(
            fixture,
            "typeName",
            &format!("({TYPE})Ljava/lang/String;"),
            vec![Value::Ref(ty)],
        )
        .expect("getTypeName succeeds"),
    )
}

fn object_text(fixture: &Fixture, value: ObjRef) -> String {
    text(
        fixture,
        call(
            fixture,
            "objectText",
            "(Ljava/lang/Object;)Ljava/lang/String;",
            vec![Value::Ref(value)],
        )
        .expect("toString succeeds"),
    )
}

fn runtime_class_name(fixture: &Fixture, ty: ObjRef) -> String {
    text(
        fixture,
        call(
            fixture,
            "runtimeClassName",
            &format!("({TYPE})Ljava/lang/String;"),
            vec![Value::Ref(ty)],
        )
        .expect("getClass().getName() succeeds"),
    )
}

#[test]
fn class_generic_superclass_and_interfaces() {
    let fixture = fixture();

    // Box's own superclass is raw Object; a generic signature with a raw superclass still
    // reports a Class object there.
    let superclass = super_type(&fixture, fixture.box_object);
    assert_eq!(class_name(&fixture, superclass), "java.lang.Object");
    assert!(matches!(
        fixture.shared.heap().get(superclass).expect("live"),
        HeapObject::Class(_)
    ));

    // StringBox extends Box<Integer>: a ParameterizedType whose raw type is Box and whose
    // one argument is the Integer Class object.
    let string_box_generic =
        parameterized(&fixture, super_type(&fixture, fixture.string_box_object));
    assert_eq!(string_box_generic.raw, ClassRef::Named(fixture.box_class));
    assert_eq!(string_box_generic.owner, None);
    assert_eq!(string_box_generic.arguments.len(), 1);
    assert_eq!(
        class_name(&fixture, string_box_generic.arguments[0]),
        "java.lang.Integer"
    );
    assert_eq!(
        type_name(&fixture, super_type(&fixture, fixture.string_box_object)),
        "demo.Box<java.lang.Integer>"
    );

    // Box implements Comparable<Box<T>>.
    // An interface reports no generic superclass, even though its signature names Object.
    let comparable_class = class_object_of(&fixture, "java/lang/Comparable");
    assert_eq!(
        reference(
            call(
                &fixture,
                "superType",
                &format!("({CLASS}){TYPE}"),
                vec![Value::Ref(comparable_class)],
            )
            .expect("getGenericSuperclass succeeds"),
        ),
        ObjRef::NULL
    );

    let comparable = parameterized(&fixture, first_interface(&fixture, fixture.box_object));
    let ClassRef::Named(comparable_class) = comparable.raw else {
        panic!("Comparable is a named class");
    };
    assert_eq!(
        fixture.shared.entry(comparable_class).expect("loaded").name,
        "java/lang/Comparable"
    );
    assert_eq!(comparable.arguments.len(), 1);
    let box_of_t = parameterized(&fixture, comparable.arguments[0]);
    assert_eq!(box_of_t.raw, ClassRef::Named(fixture.box_class));
    let variable = type_variable(&fixture, box_of_t.arguments[0]);
    assert_eq!(variable.name, "T");
    assert_eq!(
        variable.declaration,
        Some(rava_vm::TypeDeclaration::Class(fixture.box_class))
    );
    assert_eq!(bounds(&fixture, box_of_t.arguments[0]).len(), 1);
    assert_eq!(
        class_name(&fixture, bounds(&fixture, box_of_t.arguments[0])[0]),
        "java.lang.Number"
    );

    // Class.getTypeParameters on Box returns its own T, with the Number bound and the
    // class itself as the declaration.
    let parameters = reference(
        call(
            &fixture,
            "classParameters",
            &format!("({CLASS}){TYPE_VARIABLE_ARRAY}"),
            vec![Value::Ref(fixture.box_object)],
        )
        .expect("getTypeParameters succeeds"),
    );
    let parameters = elements(&fixture, parameters);
    assert_eq!(parameters.len(), 1);
    assert_eq!(variable_name(&fixture, parameters[0]), "T");
    let variable = type_variable(&fixture, parameters[0]);
    assert_eq!(
        variable.declaration,
        Some(rava_vm::TypeDeclaration::Class(fixture.box_class))
    );
    assert_eq!(variable.bounds.len(), 1);
    assert_eq!(class_name(&fixture, variable.bounds[0]), "java.lang.Number");
    assert_eq!(declaration(&fixture, parameters[0]), fixture.box_object);

    // Plain is not generic: its superclass is Object, it has no type parameters, and its
    // field type is a raw String Class.
    assert_eq!(
        class_name(&fixture, super_type(&fixture, fixture.plain_object)),
        "java.lang.Object"
    );
    let plain_parameters = reference(
        call(
            &fixture,
            "classParameters",
            &format!("({CLASS}){TYPE_VARIABLE_ARRAY}"),
            vec![Value::Ref(fixture.plain_object)],
        )
        .expect("getTypeParameters succeeds"),
    );
    assert!(elements(&fixture, plain_parameters).is_empty());
    let name = field_type(&fixture, fixture.plain_object, "name");
    assert_eq!(class_name(&fixture, name), "java.lang.String");
}

#[test]
fn field_generic_types() {
    let fixture = fixture();
    let box_object = fixture.box_object;

    // List<String>: a parameterized type with a top-level (ownerless) raw List.
    let labels = field_type(&fixture, box_object, "labels");
    let labels = parameterized(&fixture, labels);
    assert_eq!(labels.arguments.len(), 1);
    assert_eq!(labels.owner, None);
    assert_eq!(
        class_name(&fixture, labels.arguments[0]),
        "java.lang.String"
    );
    assert_eq!(
        type_name(&fixture, field_type(&fixture, box_object, "labels")),
        "java.util.List<java.lang.String>"
    );

    // T[]: a generic array whose component is the class type variable T.
    let values = field_type(&fixture, box_object, "values");
    let TypeObject::GenericArray(array) = type_object(&fixture, values) else {
        panic!("T[] is a GenericArrayType");
    };
    let component = type_variable(&fixture, array.component);
    assert_eq!(component.name, "T");
    assert_eq!(
        component.declaration,
        Some(rava_vm::TypeDeclaration::Class(fixture.box_class))
    );
    assert_eq!(type_name(&fixture, values), "T[]");
    // The structural cache makes the field's T the same object `getTypeParameters` returns.
    let class_parameters = reference(
        call(
            &fixture,
            "classParameters",
            &format!("({CLASS}){TYPE_VARIABLE_ARRAY}"),
            vec![Value::Ref(fixture.box_object)],
        )
        .expect("getTypeParameters succeeds"),
    );
    assert_eq!(elements(&fixture, class_parameters)[0], array.component);

    // List<? extends Number>: the argument is a WildcardType with upper Number, no lower.
    let upper = field_type(&fixture, box_object, "upper");
    let upper_arguments = type_arguments(&fixture, upper);
    assert_eq!(upper_arguments.len(), 1);
    let upper_wildcard = wildcard(&fixture, upper_arguments[0]);
    assert_eq!(upper_wildcard.upper.len(), 1);
    assert!(upper_wildcard.lower.is_empty());
    assert_eq!(
        class_name(&fixture, upper_wildcard.upper[0]),
        "java.lang.Number"
    );
    assert_eq!(
        type_name(&fixture, upper_arguments[0]),
        "? extends java.lang.Number"
    );

    // List<? super Integer>: upper Object, lower Integer.
    let lower = field_type(&fixture, box_object, "lower");
    let lower_arguments = type_arguments(&fixture, lower);
    let lower_wildcard = wildcard(&fixture, lower_arguments[0]);
    assert_eq!(lower_wildcard.upper.len(), 1);
    assert_eq!(
        class_name(&fixture, lower_wildcard.upper[0]),
        "java.lang.Object"
    );
    assert_eq!(
        class_name(&fixture, lower_wildcard.lower[0]),
        "java.lang.Integer"
    );
    assert_eq!(
        type_name(&fixture, lower_arguments[0]),
        "? super java.lang.Integer"
    );

    // Map$Entry<String, Integer>: a raw member class whose owner is Map, derived from the
    // class's InnerClasses entry even though the signature has no `.Owner` suffix.
    let entry = parameterized(&fixture, field_type(&fixture, box_object, "entry"));
    let ClassRef::Named(entry_class) = entry.raw else {
        panic!("Map$Entry is a named class");
    };
    assert_eq!(
        fixture.shared.entry(entry_class).expect("loaded").name,
        "java/util/Map$Entry"
    );
    assert_eq!(entry.arguments.len(), 2);
    assert_eq!(class_name(&fixture, entry.arguments[0]), "java.lang.String");
    assert_eq!(
        class_name(&fixture, entry.arguments[1]),
        "java.lang.Integer"
    );
    let owner = entry.owner.expect("a member class has an owner");
    assert_eq!(class_name(&fixture, owner), "java.util.Map");

    // Nest<String>.Inner<Integer>: an explicitly owned parameterized type.
    let inner = parameterized(&fixture, field_type(&fixture, fixture.nest_object, "inner"));
    let ClassRef::Named(inner_class) = inner.raw else {
        panic!("Inner is a named class");
    };
    assert_eq!(
        fixture.shared.entry(inner_class).expect("loaded").name,
        "demo/Nest$Inner"
    );
    let owner = parameterized(&fixture, inner.owner.expect("Inner has an owner"));
    let ClassRef::Named(owner_class) = owner.raw else {
        panic!("Nest is a named class");
    };
    assert_eq!(
        fixture.shared.entry(owner_class).expect("loaded").name,
        "demo/Nest"
    );
    assert_eq!(owner.arguments.len(), 1);
    assert_eq!(class_name(&fixture, owner.arguments[0]), "java.lang.String");
    assert_eq!(inner.arguments.len(), 1);
    assert_eq!(
        class_name(&fixture, inner.arguments[0]),
        "java.lang.Integer"
    );
}

#[test]
fn method_generic_signatures() {
    let fixture = fixture();
    let box_object = fixture.box_object;
    let list = class_object_of(&fixture, "java/util/List");

    // <E extends CharSequence> List<E> copy(List<E>): one method type parameter, shared
    // between the return type and the parameter type.
    let return_type_ref = return_type(&fixture, box_object, "copy", &[list]);
    let parameters = parameter_types(&fixture, box_object, "copy", &[list]);
    assert_eq!(parameters.len(), 1);
    assert_eq!(return_type_ref, parameters[0]);
    let return_parameterized = parameterized(&fixture, return_type_ref);
    assert_eq!(return_parameterized.arguments.len(), 1);
    let return_variable = type_variable(&fixture, return_parameterized.arguments[0]);
    assert_eq!(return_variable.name, "E");
    assert_eq!(return_variable.bounds.len(), 1);
    assert_eq!(
        class_name(&fixture, return_variable.bounds[0]),
        "java.lang.CharSequence"
    );
    let copy = method_reference(&fixture, "copy", "(Ljava/util/List;)Ljava/util/List;");
    assert_eq!(
        return_variable.declaration,
        Some(rava_vm::TypeDeclaration::Member {
            kind: MemberKind::Method,
            class: fixture.box_class,
            name: "copy".to_owned(),
            descriptor: "(Ljava/util/List;)Ljava/util/List;".to_owned(),
        })
    );
    assert_eq!(
        declaration(&fixture, return_parameterized.arguments[0]),
        copy
    );

    let variables = method_parameters(&fixture, box_object, "copy", &[list]);
    assert_eq!(variables.len(), 1);
    assert_eq!(variable_name(&fixture, variables[0]), "E");
    assert_eq!(variables[0], return_parameterized.arguments[0]);

    // compareTo(Box<T>): the parameter is a parameterized Box whose argument is the
    // class-level T.
    let compare_to = parameter_types(
        &fixture,
        box_object,
        "compareTo",
        &[class_object_of(&fixture, "demo/Box")],
    );
    assert_eq!(compare_to.len(), 1);
    let box_of_t = parameterized(&fixture, compare_to[0]);
    assert_eq!(box_of_t.raw, ClassRef::Named(fixture.box_class));
    let variable = type_variable(&fixture, box_of_t.arguments[0]);
    assert_eq!(variable.name, "T");
    assert_eq!(
        variable.declaration,
        Some(rava_vm::TypeDeclaration::Class(fixture.box_class))
    );

    // value() returns the class-level T; size() returns primitive int.
    let value = type_variable(&fixture, return_type(&fixture, box_object, "value", &[]));
    assert_eq!(value.name, "T");
    assert_eq!(
        class_name(&fixture, return_type(&fixture, box_object, "size", &[])),
        "int"
    );
}

#[test]
fn constructor_generic_signatures() {
    let fixture = fixture();
    let box_object = fixture.box_object;
    let list = class_object_of(&fixture, "java/util/List");
    let number = class_object_of(&fixture, "java/lang/Number");

    // <C extends Number> Box(List<C>): a constructor-level type variable whose declaration
    // is the Constructor object.
    let parameters = constructor_parameter_types(&fixture, box_object, &[list]);
    assert_eq!(parameters.len(), 1);
    let parameterized_type = parameterized(&fixture, parameters[0]);
    let variable = type_variable(&fixture, parameterized_type.arguments[0]);
    assert_eq!(variable.name, "C");
    assert_eq!(variable.bounds.len(), 1);
    assert_eq!(class_name(&fixture, variable.bounds[0]), "java.lang.Number");
    let constructor = constructor_reference(&fixture, "(Ljava/util/List;)V");
    assert_eq!(
        declaration(&fixture, parameterized_type.arguments[0]),
        constructor
    );
    let variables = constructor_parameters(&fixture, box_object, &[list]);
    assert_eq!(variables.len(), 1);
    assert_eq!(variable_name(&fixture, variables[0]), "C");
    assert_eq!(variables[0], parameterized_type.arguments[0]);

    // Box(T): the constructor parameter uses the class-level T.
    let instance = constructor_parameter_types(&fixture, box_object, &[number]);
    assert_eq!(instance.len(), 1);
    let variable = type_variable(&fixture, instance[0]);
    assert_eq!(variable.name, "T");
    assert_eq!(
        variable.declaration,
        Some(rava_vm::TypeDeclaration::Class(fixture.box_class))
    );
    assert!(constructor_parameters(&fixture, box_object, &[number]).is_empty());

    // A constructor with no generic signature reports raw Class parameter types and no
    // type parameters.
    let plain_parameters = constructor_parameter_types(&fixture, fixture.plain_object, &[]);
    assert!(plain_parameters.is_empty());
    assert!(constructor_parameters(&fixture, fixture.plain_object, &[]).is_empty());
}

#[test]
fn type_objects_have_a_deterministic_class_identity_and_text() {
    let fixture = fixture();
    let box_object = fixture.box_object;

    let superclass = super_type(&fixture, fixture.string_box_object);
    assert_eq!(
        runtime_class_name(&fixture, superclass),
        "java.lang.reflect.ParameterizedType"
    );
    assert_eq!(
        object_text(&fixture, superclass),
        "demo.Box<java.lang.Integer>"
    );
    assert_eq!(
        type_name(&fixture, superclass),
        "demo.Box<java.lang.Integer>"
    );

    let upper = type_arguments(&fixture, field_type(&fixture, box_object, "upper"))[0];
    assert_eq!(
        runtime_class_name(&fixture, upper),
        "java.lang.reflect.WildcardType"
    );
    let values = field_type(&fixture, box_object, "values");
    assert_eq!(
        runtime_class_name(&fixture, values),
        "java.lang.reflect.GenericArrayType"
    );

    let list = class_object_of(&fixture, "java/util/List");
    let variables = method_parameters(&fixture, box_object, "copy", &[list]);
    assert_eq!(
        runtime_class_name(&fixture, variables[0]),
        "java.lang.reflect.TypeVariable"
    );
    assert_eq!(object_text(&fixture, variables[0]), "E");

    // Two reflections of one declaration share the cached Type object.
    assert_eq!(
        field_type(&fixture, box_object, "labels"),
        field_type(&fixture, box_object, "labels")
    );
    assert_eq!(
        super_type(&fixture, fixture.string_box_object),
        super_type(&fixture, fixture.string_box_object)
    );
}

#[test]
fn malformed_signature_strings_are_typed_errors() {
    assert_eq!(
        parse_class_signature("<T"),
        Err(SignatureError::UnexpectedEnd {
            offset: 2,
            expected: "':' after a type-parameter name",
        })
    );
    assert!(matches!(
        parse_class_signature("Lfoo"),
        Err(SignatureError::UnexpectedEnd { offset: 4, .. })
    ));
    assert!(matches!(
        parse_field_signature("Ljava/util/List<;"),
        Err(SignatureError::UnexpectedCharacter { offset: 16, .. })
    ));
    assert!(matches!(
        parse_field_signature("I"),
        Err(SignatureError::UnexpectedCharacter { offset: 0, .. })
    ));
    assert_eq!(
        parse_field_signature("V"),
        Err(SignatureError::UnexpectedVoid { offset: 0 })
    );
    assert!(matches!(
        parse_field_signature("Ljava/lang/String;;"),
        Err(SignatureError::TrailingCharacters { offset: 18, .. })
    ));
    assert!(matches!(
        parse_method_signature("(I"),
        Err(SignatureError::UnexpectedEnd { .. })
    ));
    assert!(matches!(
        parse_method_signature("()X"),
        Err(SignatureError::UnexpectedCharacter { offset: 2, .. })
    ));

    // A valid signature keeps the model's vocabulary: the argument tags survive parsing.
    let parsed = parse_field_signature("Ljava/util/List<+Ljava/lang/Number;>;").unwrap();
    let TypeSignature::Class(class) = parsed else {
        panic!("a field signature is a class type");
    };
    assert!(matches!(class.arguments[0], TypeArgument::Extends(_)));
}

/// A class source over in-memory bytes, for the malformed-signature class.
#[derive(Debug)]
struct MapSource {
    entries: HashMap<String, Vec<u8>>,
}

impl MapSource {
    fn new(entries: Vec<(&str, Vec<u8>)>) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|(name, bytes)| (name.to_owned(), bytes))
                .collect(),
        }
    }
}

impl rava_vm::ClassSource for MapSource {
    fn read(&self, binary_name: &str) -> Result<Option<Vec<u8>>, VmError> {
        Ok(self.entries.get(binary_name).cloned())
    }
}

#[test]
fn a_malformed_class_signature_is_a_typed_reflection_error() {
    let bytes = std::fs::read(fixture_classes().join("demo/Box.class")).expect("Box.class reads");
    let file = ClassFile::parse(&bytes).expect("Box.class parses");
    let signature = file
        .metadata()
        .expect("Box metadata decodes")
        .signature
        .expect("Box is generic");
    let position = bytes
        .windows(signature.len())
        .position(|window| window == signature.as_bytes())
        .expect("the signature string is in the constant pool");
    let mut broken = bytes.clone();
    let last = position + signature.len() - 1;
    assert_eq!(broken[last], b';');
    // Same length, still valid UTF-8, and no longer a class signature.
    broken[last] = b'!';

    let shared = Shared::with_library_dir(c1_classes());
    instance::register_natives(&shared);
    object::register_natives(&shared);
    reflection::register_natives(&shared);
    generic::register_natives(&shared);
    let loader = shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(MapSource::new(vec![("demo/Box", broken)])),
        )
        .expect("the broken loader is added");
    let class = shared
        .load_class(loader, "demo/Box")
        .expect("the broken class defines");
    let class_object = shared.class_object(class).expect("its class object");
    let class_class = shared
        .load_library_class("java/lang/Class")
        .expect("java.lang.Class loads");
    let thread = shared.thread_state(2);
    let error = run_instance_method(
        &shared,
        &thread,
        class_class,
        "getGenericSuperclass",
        "()Ljava/lang/reflect/Type;",
        class_object,
        Vec::new(),
    )
    .expect_err("the malformed signature is rejected");
    match error {
        InterpError::Vm(VmError::Signature(SignatureError::UnexpectedCharacter {
            offset,
            found,
            ..
        })) => {
            assert_eq!(found, '!');
            assert_eq!(offset, signature.len() - 1);
        }
        other => panic!("expected a typed Signature error, found {other:?}"),
    }
}
