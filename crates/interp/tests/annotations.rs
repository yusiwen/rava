//! M2.11 acceptance: annotation reflection and annotation objects.
//!
//! The fixture is a set of classes compiled at release 17 and committed under
//! `tests/fixtures/annotations/classes`; the test loads them through a `DirectorySource`
//! child loader, so no JDK is needed at test time. `demo.Annotated` and its superclass
//! `demo.Base` carry `@demo.Mark` (string, int, boolean, long, double, class literal,
//! enum, nested annotation, `String[]` and `int[]` elements, each with a default) and the
//! `@Inherited` annotation
//! `@demo.InheritedMark` on a class, a field, a method and a constructor. Regenerate the
//! classes with `javac --release 17 -d classes src/demo/*.java` from the fixture
//! directory; the test only reads the committed bytes.
//!
//! The assertions go through the real `Class` and `Field`/`Method`/`Constructor` stubs and
//! the interpreter's `invokevirtual` path, with a small assembled `AnnProbe` that calls an
//! element method and `annotationType` on an annotation-typed receiver. Values are checked
//! against the committed class files themselves, not against a hard-coded host JDK view.

use std::path::PathBuf;
use std::sync::Arc;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::run_instance_method;
use rava_vm::{
    ArrayKind, ClassId, DirectorySource, LoaderId, MemberKind, ObjRef, Shared, ThreadState, Value,
    annotation, instance, object, reflection, string,
};

const ACC_PUBLIC_STATIC: u16 = 0x0009;
const ANNOTATION_ARRAY: &str = "()[Ljava/lang/annotation/Annotation;";
const GET_ANNOTATION: &str = "(Ljava/lang/Class;)Ljava/lang/annotation/Annotation;";

const STRING_ELEMENT: &str = "()Ljava/lang/String;";
const INT_ELEMENT: &str = "()I";
const CLASS_ELEMENT: &str = "()Ljava/lang/Class;";
const COLOR_ELEMENT: &str = "()Ldemo/Color;";
const NESTED_ELEMENT: &str = "()Ldemo/Nested;";
const ALIASES_ELEMENT: &str = "()[Ljava/lang/String;";
const PRIMES_ELEMENT: &str = "()[I";
const BOOLEAN_ELEMENT: &str = "()Z";
const LONG_ELEMENT: &str = "()J";
const DOUBLE_ELEMENT: &str = "()D";

fn fixture_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/annotations/classes")
}

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

struct Fixture {
    shared: Shared,
    loader: LoaderId,
    annotated_object: ObjRef,
    sub_object: ObjRef,
    mark: ClassId,
    mark_object: ObjRef,
    inherited_mark: ClassId,
    nested: ClassId,
    class_class: ClassId,
    field_class: ClassId,
    method_class: ClassId,
    constructor_class: ClassId,
    accessible_class: ClassId,
    enum_class: ClassId,
    probe: ClassId,
    thread: ThreadState,
}

/// `AnnProbe`: bytecode that calls a method on an annotation-typed receiver, so the
/// interpreter's annotation dispatch (not only a direct native call) is exercised.
fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("AnnProbe", "java/lang/Object", 61);
    let mark_name = builder.method_ref("demo/Mark", "name", "()Ljava/lang/String;");
    let mark_count = builder.method_ref("demo/Mark", "count", "()I");
    let annotation_type = builder.method_ref(
        "java/lang/annotation/Annotation",
        "annotationType",
        "()Ljava/lang/Class;",
    );
    let object_to_string =
        builder.method_ref("java/lang/Object", "toString", "()Ljava/lang/String;");
    let enum_name = builder.method_ref("java/lang/Enum", "name", "()Ljava/lang/String;");
    let object_get_class =
        builder.method_ref("java/lang/Object", "getClass", "()Ljava/lang/Class;");

    // static String elementName(java.lang.annotation.Annotation a) { return a.name(); }
    let mut element_name = Assembler::new();
    element_name.aload(0);
    element_name.invokevirtual(mark_name);
    element_name.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "elementName",
        "(Ljava/lang/annotation/Annotation;)Ljava/lang/String;",
        Some((1, 1, element_name.finish().unwrap())),
    );

    // static int elementCount(demo.Mark mark) { return mark.count(); }
    let mut element_count = Assembler::new();
    element_count.aload(0);
    element_count.invokevirtual(mark_count);
    element_count.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "elementCount",
        "(Ldemo/Mark;)I",
        Some((1, 1, element_count.finish().unwrap())),
    );

    // static Class annotationType(Annotation a) { return a.annotationType(); }
    let mut type_method = Assembler::new();
    type_method.aload(0);
    type_method.invokevirtual(annotation_type);
    type_method.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "annotationType",
        "(Ljava/lang/annotation/Annotation;)Ljava/lang/Class;",
        Some((1, 1, type_method.finish().unwrap())),
    );

    // static String annotationText(Annotation a) { return a.toString(); }
    let mut text = Assembler::new();
    text.aload(0);
    text.invokevirtual(object_to_string);
    text.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "annotationText",
        "(Ljava/lang/annotation/Annotation;)Ljava/lang/String;",
        Some((1, 1, text.finish().unwrap())),
    );

    // static String enumName(demo.Color color) { return color.name(); }
    let mut name = Assembler::new();
    name.aload(0);
    name.invokevirtual(enum_name);
    name.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "enumName",
        "(Ldemo/Color;)Ljava/lang/String;",
        Some((1, 1, name.finish().unwrap())),
    );

    // static Class enumClass(demo.Color color) { return color.getClass(); }
    let mut class_of = Assembler::new();
    class_of.aload(0);
    class_of.invokevirtual(object_get_class);
    class_of.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "enumClass",
        "(Ldemo/Color;)Ljava/lang/Class;",
        Some((1, 1, class_of.finish().unwrap())),
    );

    builder.build()
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    instance::register_natives(&shared);
    object::register_natives(&shared);
    string::register_natives(&shared);
    reflection::register_natives(&shared);
    annotation::register_natives(&shared);
    let loader = shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(fixture_classes())),
        )
        .expect("the fixture loader is added");
    let annotated = shared
        .load_class(loader, "demo/Annotated")
        .expect("demo/Annotated loads");
    let sub = shared
        .load_class(loader, "demo/Sub")
        .expect("demo/Sub loads");
    let mark = shared
        .load_class(loader, "demo/Mark")
        .expect("demo/Mark loads");
    let inherited_mark = shared
        .load_class(loader, "demo/InheritedMark")
        .expect("demo/InheritedMark loads");
    let nested = shared
        .load_class(loader, "demo/Nested")
        .expect("demo/Nested loads");
    let annotated_object = shared
        .class_object(annotated)
        .expect("demo/Annotated gets a class object");
    let sub_object = shared
        .class_object(sub)
        .expect("demo/Sub gets a class object");
    let mark_object = shared
        .class_object(mark)
        .expect("demo/Mark gets a class object");
    let class_class = shared
        .load_library_class("java/lang/Class")
        .expect("java/lang/Class loads");
    let field_class = shared
        .load_library_class("java/lang/reflect/Field")
        .expect("java.lang.reflect.Field loads");
    let method_class = shared
        .load_library_class("java/lang/reflect/Method")
        .expect("java.lang.reflect.Method loads");
    let constructor_class = shared
        .load_library_class("java/lang/reflect/Constructor")
        .expect("java.lang.reflect.Constructor loads");
    let accessible_class = shared
        .load_library_class("java/lang/reflect/AccessibleObject")
        .expect("java.lang.reflect.AccessibleObject loads");
    let enum_class = shared
        .load_library_class("java/lang/Enum")
        .expect("java.lang.Enum loads");
    let probe = shared
        .define_class(loader, "AnnProbe", &probe_bytes())
        .expect("AnnProbe defines");
    let thread = shared.thread_state(1);
    Fixture {
        shared,
        loader,
        annotated_object,
        sub_object,
        mark,
        mark_object,
        inherited_mark,
        nested,
        class_class,
        field_class,
        method_class,
        constructor_class,
        accessible_class,
        enum_class,
        probe,
        thread,
    }
}

fn ref_result(value: Option<Value>) -> ObjRef {
    match value {
        Some(Value::Ref(reference)) => reference,
        other => panic!("expected a reference result, found {other:?}"),
    }
}

fn array_refs(shared: &Shared, value: Option<Value>) -> Vec<ObjRef> {
    let reference = ref_result(value);
    let heap = shared.heap();
    let array = heap.array(reference).expect("a reference array");
    assert_eq!(array.kind, ArrayKind::Reference);
    array
        .elements
        .iter()
        .map(|value| match value {
            Value::Ref(reference) => *reference,
            other => panic!("expected a reference array element, found {other:?}"),
        })
        .collect()
}

fn array_ints(shared: &Shared, value: Option<Value>) -> Vec<i32> {
    let reference = ref_result(value);
    let heap = shared.heap();
    let array = heap.array(reference).expect("an int array");
    assert_eq!(array.kind, ArrayKind::Int);
    array
        .elements
        .iter()
        .map(|value| match value {
            Value::Int(value) => *value,
            other => panic!("expected an int array element, found {other:?}"),
        })
        .collect()
}

fn string_of(shared: &Shared, value: Option<Value>) -> String {
    let reference = ref_result(value);
    let heap = shared.heap();
    heap.string(reference)
        .expect("the result is a string")
        .to_owned()
}

fn int_of(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int result, found {other:?}"),
    }
}

fn annotation_class_of(fixture: &Fixture, annotation: ObjRef) -> ClassId {
    let heap = fixture.shared.heap();
    heap.annotation(annotation)
        .expect("the result is an annotation object")
        .class
}

/// Call a method on an annotation instance through the interpreter, which dispatches the
/// receiver to its annotation type.
fn call_member(
    fixture: &Fixture,
    annotation: ObjRef,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Option<Value> {
    let class = annotation_class_of(fixture, annotation);
    run_instance_method(
        &fixture.shared,
        &fixture.thread,
        class,
        name,
        descriptor,
        annotation,
        args,
    )
    .unwrap_or_else(|error| panic!("{name}{descriptor} on an annotation fails: {error}"))
}

fn element(fixture: &Fixture, annotation: ObjRef, name: &str, descriptor: &str) -> Option<Value> {
    call_member(fixture, annotation, name, descriptor, Vec::new())
}

fn element_text(fixture: &Fixture, annotation: ObjRef, name: &str) -> String {
    string_of(
        &fixture.shared,
        element(fixture, annotation, name, STRING_ELEMENT),
    )
}

fn element_int(fixture: &Fixture, annotation: ObjRef, name: &str) -> i32 {
    int_of(element(fixture, annotation, name, INT_ELEMENT))
}

fn element_value(fixture: &Fixture, annotation: ObjRef, name: &str, descriptor: &str) -> Value {
    match element(fixture, annotation, name, descriptor) {
        Some(value) => value,
        None => panic!("{name}{descriptor} on an annotation returned no value"),
    }
}

fn class_query(
    fixture: &Fixture,
    receiver: ObjRef,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Option<Value> {
    run_instance_method(
        &fixture.shared,
        &fixture.thread,
        fixture.class_class,
        name,
        descriptor,
        receiver,
        args,
    )
    .unwrap_or_else(|error| panic!("Class.{name}{descriptor} fails: {error}"))
}

fn member_query(
    fixture: &Fixture,
    class: ClassId,
    receiver: ObjRef,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Option<Value> {
    run_instance_method(
        &fixture.shared,
        &fixture.thread,
        class,
        name,
        descriptor,
        receiver,
        args,
    )
    .unwrap_or_else(|error| panic!("{name}{descriptor} on a reflected member fails: {error}"))
}

fn declared(fixture: &Fixture, receiver: ObjRef) -> Vec<ObjRef> {
    array_refs(
        &fixture.shared,
        class_query(
            fixture,
            receiver,
            "getDeclaredAnnotations",
            ANNOTATION_ARRAY,
            Vec::new(),
        ),
    )
}

fn all(fixture: &Fixture, receiver: ObjRef) -> Vec<ObjRef> {
    array_refs(
        &fixture.shared,
        class_query(
            fixture,
            receiver,
            "getAnnotations",
            ANNOTATION_ARRAY,
            Vec::new(),
        ),
    )
}

fn member_declared(fixture: &Fixture, class: ClassId, receiver: ObjRef) -> Vec<ObjRef> {
    array_refs(
        &fixture.shared,
        member_query(
            fixture,
            class,
            receiver,
            "getDeclaredAnnotations",
            ANNOTATION_ARRAY,
            Vec::new(),
        ),
    )
}

fn find_annotation(fixture: &Fixture, annotations: &[ObjRef], class: ClassId) -> Option<ObjRef> {
    annotations
        .iter()
        .copied()
        .find(|annotation| annotation_class_of(fixture, *annotation) == class)
}

fn class_name(fixture: &Fixture, class_object: ObjRef) -> String {
    string_of(
        &fixture.shared,
        class_query(
            fixture,
            class_object,
            "getName",
            "()Ljava/lang/String;",
            Vec::new(),
        ),
    )
}

fn member(fixture: &Fixture, kind: MemberKind, name: &str, descriptor: &str) -> ObjRef {
    let annotated = fixture
        .shared
        .heap()
        .class(fixture.annotated_object)
        .expect("the class object names demo/Annotated");
    fixture
        .shared
        .reflect_member(annotated, kind, name, descriptor)
        .expect("the member object allocates")
}

fn probe_call(fixture: &Fixture, name: &str, descriptor: &str, args: Vec<Value>) -> Option<Value> {
    rava_interp::run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        name,
        descriptor,
        args,
    )
    .unwrap_or_else(|error| panic!("AnnProbe.{name}{descriptor} fails: {error}"))
}

fn mark_annotation(fixture: &Fixture) -> ObjRef {
    let annotations = declared(fixture, fixture.annotated_object);
    find_annotation(fixture, &annotations, fixture.mark).expect("the class carries @Mark")
}

fn enum_name_of(fixture: &Fixture, constant: ObjRef) -> String {
    string_of(
        &fixture.shared,
        run_instance_method(
            &fixture.shared,
            &fixture.thread,
            fixture.enum_class,
            "name",
            "()Ljava/lang/String;",
            constant,
            Vec::new(),
        )
        .expect("Enum.name succeeds"),
    )
}

#[test]
fn class_annotations_expose_every_element_value() {
    let fixture = fixture();
    let annotations = declared(&fixture, fixture.annotated_object);
    assert_eq!(annotations.len(), 2, "class-file annotation order");

    let mark = find_annotation(&fixture, &annotations, fixture.mark).expect("@Mark is present");
    assert_eq!(
        annotation_class_of(&fixture, mark),
        fixture.mark,
        "the annotation object names its annotation type"
    );

    // annotationType() identity: the same class object the class table hands out.
    let annotation_type = ref_result(call_member(
        &fixture,
        mark,
        "annotationType",
        "()Ljava/lang/Class;",
        Vec::new(),
    ));
    assert_eq!(annotation_type, fixture.mark_object);

    // String and int elements keep their exact descriptors.
    assert_eq!(element_text(&fixture, mark, "name"), "class");
    assert_eq!(element_int(&fixture, mark, "count"), 7);
    assert_eq!(
        element_value(&fixture, mark, "flag", BOOLEAN_ELEMENT),
        Value::Int(0),
        "a boolean element is false"
    );
    assert_eq!(
        element_value(&fixture, mark, "big", LONG_ELEMENT),
        Value::Long(9_000_000_000)
    );
    assert_eq!(
        element_value(&fixture, mark, "ratio", DOUBLE_ELEMENT),
        Value::Double(2.25)
    );

    // `c` element value: a class literal is a real Class object.
    let literal = ref_result(element(&fixture, mark, "type", CLASS_ELEMENT));
    assert_eq!(class_name(&fixture, literal), "java.lang.String");

    // `e` element value: an object of the enum type, not a string.
    let color = ref_result(element(&fixture, mark, "color", COLOR_ELEMENT));
    let color_class = fixture
        .shared
        .get_class(color)
        .expect("getClass on an enum");
    assert_eq!(class_name(&fixture, color_class), "demo.Color");
    assert_eq!(enum_name_of(&fixture, color), "GREEN");

    // `@` element value: a nested annotation object with its own type and element.
    let nested = ref_result(element(&fixture, mark, "nested", NESTED_ELEMENT));
    assert_eq!(annotation_class_of(&fixture, nested), fixture.nested);
    assert_eq!(element_text(&fixture, nested, "value"), "n");

    // `[` element values: reference and primitive arrays.
    assert_eq!(
        array_refs(
            &fixture.shared,
            element(&fixture, mark, "aliases", ALIASES_ELEMENT)
        )
        .into_iter()
        .map(|value| string_of(&fixture.shared, Some(Value::Ref(value))))
        .collect::<Vec<_>>(),
        vec!["k".to_owned(), "it".to_owned()]
    );
    assert_eq!(
        array_ints(
            &fixture.shared,
            element(&fixture, mark, "primes", PRIMES_ELEMENT)
        ),
        vec![5, 7]
    );
}

#[test]
fn omitted_elements_return_annotation_defaults() {
    let fixture = fixture();
    let field = member(&fixture, MemberKind::Field, "value", "I");
    let annotations = member_declared(&fixture, fixture.field_class, field);
    assert_eq!(annotations.len(), 1);
    let mark = annotations[0];
    assert_eq!(element_text(&fixture, mark, "name"), "field");

    // Every element the class file omits comes from the annotation type's
    // `AnnotationDefault`, exactly as the M2.1 model decoded it.
    assert_eq!(element_int(&fixture, mark, "count"), 3);
    let literal = ref_result(element(&fixture, mark, "type", CLASS_ELEMENT));
    assert_eq!(class_name(&fixture, literal), "java.lang.String");
    let color = ref_result(element(&fixture, mark, "color", COLOR_ELEMENT));
    let color_class = fixture
        .shared
        .get_class(color)
        .expect("getClass on an enum");
    assert_eq!(class_name(&fixture, color_class), "demo.Color");
    assert_eq!(enum_name_of(&fixture, color), "RED");
    let nested = ref_result(element(&fixture, mark, "nested", NESTED_ELEMENT));
    assert_eq!(annotation_class_of(&fixture, nested), fixture.nested);
    assert_eq!(element_text(&fixture, nested, "value"), "inner");
    assert_eq!(
        array_refs(
            &fixture.shared,
            element(&fixture, mark, "aliases", ALIASES_ELEMENT)
        )
        .into_iter()
        .map(|value| string_of(&fixture.shared, Some(Value::Ref(value))))
        .collect::<Vec<_>>(),
        vec!["a".to_owned()]
    );
    assert_eq!(
        array_ints(
            &fixture.shared,
            element(&fixture, mark, "primes", PRIMES_ELEMENT)
        ),
        vec![2, 3]
    );
}

#[test]
fn field_method_and_constructor_report_their_own_annotations() {
    let fixture = fixture();

    let field = member(&fixture, MemberKind::Field, "value", "I");
    let field_annotations = member_declared(&fixture, fixture.field_class, field);
    assert_eq!(field_annotations.len(), 1);
    assert_eq!(
        element_text(&fixture, field_annotations[0], "name"),
        "field"
    );

    let method = member(&fixture, MemberKind::Method, "run", "()V");
    let method_annotations = member_declared(&fixture, fixture.method_class, method);
    assert_eq!(method_annotations.len(), 1);
    assert_eq!(
        element_text(&fixture, method_annotations[0], "name"),
        "method"
    );
    assert_eq!(element_int(&fixture, method_annotations[0], "count"), 1);

    let constructor = member(&fixture, MemberKind::Constructor, "<init>", "()V");
    let constructor_annotations = member_declared(&fixture, fixture.constructor_class, constructor);
    assert_eq!(constructor_annotations.len(), 1);
    assert_eq!(
        element_text(&fixture, constructor_annotations[0], "name"),
        "ctor"
    );

    // An unannotated field returns an empty array, not null.
    let plain = member(&fixture, MemberKind::Field, "plain", "Ljava/lang/String;");
    assert!(member_declared(&fixture, fixture.field_class, plain).is_empty());

    // `getAnnotations` on a member is the declared view; a member has no superclass. The
    // declaration the C1 stub resolves it to is `AccessibleObject`'s.
    let inherited = array_refs(
        &fixture.shared,
        member_query(
            &fixture,
            fixture.accessible_class,
            field,
            "getAnnotations",
            ANNOTATION_ARRAY,
            Vec::new(),
        ),
    );
    assert_eq!(inherited.len(), 1);
}

#[test]
fn get_annotation_finds_the_type_and_returns_null_when_missing() {
    let fixture = fixture();
    let nested_object = fixture
        .shared
        .class_object(fixture.nested)
        .expect("demo/Nested gets a class object");

    let found = ref_result(class_query(
        &fixture,
        fixture.annotated_object,
        "getAnnotation",
        GET_ANNOTATION,
        vec![Value::Ref(fixture.mark_object)],
    ));
    assert_eq!(annotation_class_of(&fixture, found), fixture.mark);
    assert_eq!(element_text(&fixture, found, "name"), "class");

    let absent = class_query(
        &fixture,
        fixture.annotated_object,
        "getAnnotation",
        GET_ANNOTATION,
        vec![Value::Ref(nested_object)],
    );
    assert!(ref_result(absent).is_null(), "a missing annotation is null");

    // The same query on a member, and the member's missing case.
    let field = member(&fixture, MemberKind::Field, "value", "I");
    let field_found = ref_result(member_query(
        &fixture,
        fixture.field_class,
        field,
        "getAnnotation",
        GET_ANNOTATION,
        vec![Value::Ref(fixture.mark_object)],
    ));
    assert_eq!(element_text(&fixture, field_found, "name"), "field");
    let field_absent = member_query(
        &fixture,
        fixture.field_class,
        field,
        "getAnnotation",
        GET_ANNOTATION,
        vec![Value::Ref(nested_object)],
    );
    assert!(ref_result(field_absent).is_null());
}

#[test]
fn enums_are_typed_and_stable() {
    let fixture = fixture();
    let mark = mark_annotation(&fixture);
    let first = ref_result(element(&fixture, mark, "color", COLOR_ELEMENT));
    let second = ref_result(element(&fixture, mark, "color", COLOR_ELEMENT));
    assert_eq!(first, second, "one cached object per enum constant");

    let class = fixture
        .shared
        .get_class(first)
        .expect("getClass on an enum");
    assert_eq!(class_name(&fixture, class), "demo.Color");
    assert_eq!(enum_name_of(&fixture, first), "GREEN");
}

#[test]
fn annotation_equals_hash_and_to_string_are_deterministic() {
    let fixture = fixture();
    let first = mark_annotation(&fixture);
    let second = mark_annotation(&fixture);

    assert_eq!(
        int_of(call_member(
            &fixture,
            first,
            "equals",
            "(Ljava/lang/Object;)Z",
            vec![Value::Ref(second)],
        )),
        1,
        "two reads of one class file's annotation are equal"
    );

    let hash_first = int_of(call_member(&fixture, first, "hashCode", "()I", Vec::new()));
    let hash_second = int_of(call_member(&fixture, second, "hashCode", "()I", Vec::new()));
    assert_eq!(hash_first, hash_second);

    // A different annotation of the same type (the field's `@Mark`) is not equal.
    let field = member(&fixture, MemberKind::Field, "value", "I");
    let field_annotation = member_declared(&fixture, fixture.field_class, field)[0];
    assert_eq!(
        int_of(call_member(
            &fixture,
            first,
            "equals",
            "(Ljava/lang/Object;)Z",
            vec![Value::Ref(field_annotation)],
        )),
        0
    );

    let text = string_of(
        &fixture.shared,
        call_member(
            &fixture,
            first,
            "toString",
            "()Ljava/lang/String;",
            Vec::new(),
        ),
    );
    assert!(
        text.starts_with("@demo.Mark("),
        "Annotation.toString names the type: {text}"
    );
    assert!(
        text.contains("name=\"class\""),
        "toString has the elements: {text}"
    );
    assert!(!text.contains("count=3"), "toString omits defaults: {text}");
}

#[test]
fn class_get_annotations_includes_inherited_annotations() {
    let fixture = fixture();

    let sub_declared = declared(&fixture, fixture.sub_object);
    assert_eq!(sub_declared.len(), 1, "Sub declares only @Mark");
    assert_eq!(annotation_class_of(&fixture, sub_declared[0]), fixture.mark);

    let sub_all = all(&fixture, fixture.sub_object);
    assert_eq!(sub_all.len(), 2, "Sub inherits @InheritedMark");
    let inherited = find_annotation(&fixture, &sub_all, fixture.inherited_mark)
        .expect("the inherited annotation is present");
    assert_eq!(element_text(&fixture, inherited, "value"), "own");

    // `Annotated` declares `@InheritedMark("own")`, so the superclass' "base" is shadowed.
    let annotated_all = all(&fixture, fixture.annotated_object);
    assert_eq!(annotated_all.len(), 2);
    let own = find_annotation(&fixture, &annotated_all, fixture.inherited_mark)
        .expect("@InheritedMark is present");
    assert_eq!(element_text(&fixture, own, "value"), "own");

    // `getAnnotation` on a class follows the same inherited view.
    let inherited_object = fixture
        .shared
        .class_object(fixture.inherited_mark)
        .expect("demo/InheritedMark gets a class object");
    let found = ref_result(class_query(
        &fixture,
        fixture.sub_object,
        "getAnnotation",
        GET_ANNOTATION,
        vec![Value::Ref(inherited_object)],
    ));
    assert_eq!(annotation_class_of(&fixture, found), fixture.inherited_mark);
}

#[test]
fn invokevirtual_dispatches_annotations_to_their_annotation_type() {
    let fixture = fixture();
    let mark = mark_annotation(&fixture);

    let name = probe_call(
        &fixture,
        "elementName",
        "(Ljava/lang/annotation/Annotation;)Ljava/lang/String;",
        vec![Value::Ref(mark)],
    );
    assert_eq!(string_of(&fixture.shared, name), "class");

    let count = probe_call(
        &fixture,
        "elementCount",
        "(Ldemo/Mark;)I",
        vec![Value::Ref(mark)],
    );
    assert_eq!(int_of(count), 7);

    let annotation_type = ref_result(probe_call(
        &fixture,
        "annotationType",
        "(Ljava/lang/annotation/Annotation;)Ljava/lang/Class;",
        vec![Value::Ref(mark)],
    ));
    assert_eq!(annotation_type, fixture.mark_object);

    let text = string_of(
        &fixture.shared,
        probe_call(
            &fixture,
            "annotationText",
            "(Ljava/lang/annotation/Annotation;)Ljava/lang/String;",
            vec![Value::Ref(mark)],
        ),
    );
    assert!(text.starts_with("@demo.Mark("));

    let color = ref_result(element(&fixture, mark, "color", COLOR_ELEMENT));
    let enum_name = probe_call(
        &fixture,
        "enumName",
        "(Ldemo/Color;)Ljava/lang/String;",
        vec![Value::Ref(color)],
    );
    assert_eq!(string_of(&fixture.shared, enum_name), "GREEN");
    let enum_class = ref_result(probe_call(
        &fixture,
        "enumClass",
        "(Ldemo/Color;)Ljava/lang/Class;",
        vec![Value::Ref(color)],
    ));
    assert_eq!(class_name(&fixture, enum_class), "demo.Color");
}

#[test]
fn repeated_queries_return_the_same_values() {
    let fixture = fixture();
    let first = declared(&fixture, fixture.annotated_object);
    let second = declared(&fixture, fixture.annotated_object);
    assert_eq!(first.len(), second.len());
    for (left, right) in first.iter().zip(&second) {
        assert_eq!(
            annotation_class_of(&fixture, *left),
            annotation_class_of(&fixture, *right)
        );
        assert_eq!(
            int_of(call_member(
                &fixture,
                *left,
                "equals",
                "(Ljava/lang/Object;)Z",
                vec![Value::Ref(*right)],
            )),
            1
        );
        assert_eq!(
            ref_result(call_member(
                &fixture,
                *left,
                "annotationType",
                "()Ljava/lang/Class;",
                Vec::new(),
            )),
            ref_result(call_member(
                &fixture,
                *right,
                "annotationType",
                "()Ljava/lang/Class;",
                Vec::new(),
            ))
        );
    }
    let _ = fixture.loader;
}

#[test]
fn annotation_receiver_is_an_annotation_object() {
    let fixture = fixture();
    let mark = mark_annotation(&fixture);
    {
        let heap = fixture.shared.heap();
        let annotation = heap.annotation(mark).expect("an annotation object");
        assert_eq!(annotation.class, fixture.mark);
        assert!(
            annotation.values.iter().any(|pair| pair.name == "name"),
            "the encoded values are stored on the object"
        );
    }

    // Object.getClass on an annotation object reports the Annotation interface, like the
    // JDK's proxy class view; annotationType() reports the annotation type.
    let annotation_class = fixture
        .shared
        .get_class(mark)
        .expect("getClass on an annotation");
    assert_eq!(
        class_name(&fixture, annotation_class),
        "java.lang.annotation.Annotation"
    );
    let _ = fixture.inherited_mark;
}
