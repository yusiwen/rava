//! M2.1 acceptance: the typed C1 metadata model and the normalized dump.
//!
//! The fixtures are committed class files compiled from `tests/fixtures/java/demo` with
//! `javac --release 17 -parameters` (see `tests/fixtures/java/build.sh`), so the suite
//! needs no JDK at test time. `demo/Kitchen.java` is the kitchen sink: every annotation
//! shape, every type-annotation path kind, a generic method, exceptions, parameter names,
//! nestmates, an inner class and an anonymous class. `demo/Circle.java` adds a record,
//! `demo/Shapes.java` a sealed hierarchy, `demo/Box.java` + `demo/IntBox.java` generic
//! signatures and a bridge method, and `demo/Defaults.java` (compiled from
//! `Annotations.java`) every `AnnotationDefault` value kind.
//!
//! The golden files are compared byte for byte. Regenerate them deliberately with
//! `RAVA_UPDATE_GOLDEN=1 cargo test -p rava-classfile --test metadata_model` and read the
//! diff before committing it; the dump is the artifact the M2.2 metadata gate will diff.

use std::path::PathBuf;

use rava_classfile::metadata::dump;
use rava_classfile::{
    Annotation, AnnotationValue, Attribute, ClassFile, ClassMetadata, Constant, ConstantValue,
    ElementValue, EnclosingMethod, Error, MethodNameAndDescriptor, MethodParameter,
    TypeAnnotationTarget, TypePathEntry, TypePathKind,
};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn parse(name: &str) -> ClassFile {
    let path = fixture_dir().join("demo").join(format!("{name}.class"));
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    ClassFile::parse(&bytes)
        .unwrap_or_else(|error| panic!("{} does not parse: {error}", path.display()))
}

fn metadata(name: &str) -> ClassMetadata {
    parse(name)
        .metadata()
        .unwrap_or_else(|error| panic!("{name} metadata: {error}"))
}

fn string(value: &str) -> ElementValue {
    ElementValue::String(value.to_owned())
}

fn annotation(type_descriptor: &str, values: Vec<(&str, ElementValue)>) -> Annotation {
    Annotation {
        type_descriptor: type_descriptor.to_owned(),
        values: values
            .into_iter()
            .map(|(name, value)| AnnotationValue {
                name: name.to_owned(),
                value,
            })
            .collect(),
    }
}

fn type_use(value: &str) -> Annotation {
    annotation("Ldemo/TypeUse;", vec![("value", string(value))])
}

fn hidden(value: &str) -> Annotation {
    annotation("Ldemo/Hidden;", vec![("value", string(value))])
}

fn path(entries: &[(TypePathKind, u8)]) -> Vec<TypePathEntry> {
    entries
        .iter()
        .map(|(kind, argument_index)| TypePathEntry {
            kind: *kind,
            argument_index: *argument_index,
        })
        .collect()
}

fn utf8_index(class: &ClassFile, text: &str) -> u16 {
    class
        .constant_pool
        .iter()
        .find_map(|(index, entry)| match entry {
            Constant::Utf8(value) if value == text => Some(index),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the constant pool has no Utf8 {text:?}"))
}

fn replace_class_attribute(class: &mut ClassFile, name: &str, data: Vec<u8>) {
    let pool = &class.constant_pool;
    let attribute = class
        .attributes
        .iter_mut()
        .find(|attribute| pool.utf8(attribute.name_index).unwrap() == name)
        .unwrap_or_else(|| panic!("the fixture has no {name} attribute"));
    attribute.data = data;
}

#[test]
fn kitchen_class_shape_is_typed() {
    let class = metadata("Kitchen");
    assert_eq!(class.binary_name, "demo/Kitchen");
    assert_eq!(class.java_name(), "demo.Kitchen");
    assert_eq!(class.major_version, 61);
    assert_eq!(class.minor_version, 0);
    assert_eq!(class.access_flags, 0x0021);
    assert!(class.is_public());
    assert!(!class.is_interface());
    assert!(!class.is_annotation_type());
    assert!(!class.is_enum());
    assert!(!class.is_record());
    assert!(!class.is_sealed());
    assert_eq!(class.super_class.as_deref(), Some("java/lang/Object"));
    assert_eq!(class.interfaces, vec!["java/lang/Comparable".to_owned()]);
    assert_eq!(
        class.signature.as_deref(),
        Some("Ljava/lang/Object;Ljava/lang/Comparable<Ldemo/Kitchen;>;")
    );
    assert_eq!(class.source_file.as_deref(), Some("Kitchen.java"));
    assert!(!class.deprecated);
    assert_eq!(class.nest_host, None);
}

#[test]
fn class_annotations_keep_declaration_order_and_every_value_kind() {
    let class = metadata("Kitchen");
    assert_eq!(class.annotations.len(), 2);
    assert_eq!(
        class.annotations[0],
        annotation(
            "Ldemo/Defaults;",
            vec![
                ("name", string("kitchen")),
                ("count", ElementValue::Int(9)),
                (
                    "color",
                    ElementValue::Enum {
                        type_descriptor: "Ldemo/Color;".to_owned(),
                        constant: "BLUE".to_owned(),
                    },
                ),
                ("type", ElementValue::Class("Ljava/lang/String;".to_owned())),
                (
                    "nested",
                    ElementValue::Annotation(annotation(
                        "Ldemo/Nested;",
                        vec![("value", string("in"))],
                    )),
                ),
                (
                    "primes",
                    ElementValue::Array(vec![
                        ElementValue::Int(2),
                        ElementValue::Int(3),
                        ElementValue::Int(5),
                        ElementValue::Int(7),
                    ]),
                ),
                (
                    "aliases",
                    ElementValue::Array(vec![string("k"), string("itchen")]),
                ),
                ("flag", ElementValue::Boolean(false)),
                ("letter", ElementValue::Char(u16::from(b'k'))),
                ("big", ElementValue::Long(9_000_000_000)),
                ("ratio", ElementValue::Double(0.5)),
                ("ratio32", ElementValue::Float(0.25)),
                ("small", ElementValue::Byte(12)),
                ("medium", ElementValue::Short(1200)),
            ]
        )
    );
    // `Annotation::value` is the accessor a later reflection layer needs; a missing pair
    // means "the default applies".
    assert_eq!(
        class.annotations[0].value("count"),
        Some(&ElementValue::Int(9))
    );
    assert_eq!(class.annotations[0].value("absent"), None);
}

#[test]
fn repeatable_annotations_stay_in_the_container_they_were_encoded_in() {
    let class = metadata("Kitchen");
    let tags = &class.annotations[1];
    assert_eq!(tags.type_descriptor, "Ldemo/Tags;");
    assert_eq!(tags.values.len(), 1);
    assert_eq!(tags.values[0].name, "value");
    let ElementValue::Array(values) = &tags.values[0].value else {
        panic!(
            "Tags.value must decode as an array, got {:?}",
            tags.values[0].value
        );
    };
    assert_eq!(
        values,
        &vec![
            ElementValue::Annotation(annotation("Ldemo/Tag;", vec![("value", string("a"))])),
            ElementValue::Annotation(annotation("Ldemo/Tag;", vec![("value", string("b"))])),
        ]
    );

    // Writing the container explicitly must decode to the same shape: the model keeps
    // what the class file encodes, it does not expand repeats.
    let io = class.method("io", "()V").expect("Kitchen.io exists");
    assert_eq!(
        io.annotations,
        vec![annotation(
            "Ldemo/Tags;",
            vec![(
                "value",
                ElementValue::Array(vec![ElementValue::Annotation(annotation(
                    "Ldemo/Tag;",
                    vec![("value", string("c"))],
                ))]),
            )],
        )]
    );
}

#[test]
fn invisible_annotations_are_typed_separately() {
    let class = metadata("Kitchen");
    assert_eq!(class.invisible_annotations, vec![hidden("class")]);
    assert_eq!(
        class
            .field("hidden")
            .expect("Kitchen.hidden exists")
            .invisible_annotations,
        vec![hidden("f")]
    );
}

#[test]
fn a_supertype_type_annotation_keeps_its_target_and_path() {
    let class = metadata("Kitchen");
    assert_eq!(class.type_annotations.len(), 1);
    let annotation = &class.type_annotations[0];
    assert_eq!(annotation.target_type, 0x10);
    assert_eq!(
        annotation.target,
        TypeAnnotationTarget::Supertype { index: 0xFFFF }
    );
    assert!(annotation.path.is_empty());
    assert_eq!(annotation.annotation, type_use("super"));
    assert!(class.invisible_type_annotations.is_empty());
}

#[test]
fn type_annotation_paths_cover_every_path_kind() {
    let class = metadata("Kitchen");

    let names = class.field("names").expect("Kitchen.names exists");
    assert_eq!(names.type_annotations.len(), 2);
    assert_eq!(names.type_annotations[0].target_type, 0x13);
    assert_eq!(
        names.type_annotations[0].target,
        TypeAnnotationTarget::Empty
    );
    assert_eq!(
        names.type_annotations[0].path,
        path(&[(TypePathKind::TypeArgument, 0)])
    );
    assert_eq!(names.type_annotations[0].annotation, type_use("arg"));
    assert!(names.type_annotations[1].path.is_empty());
    assert_eq!(names.type_annotations[1].annotation, type_use("field"));

    let tags = class.field("tags").expect("Kitchen.tags exists");
    assert_eq!(
        tags.type_annotations[0].path,
        path(&[(TypePathKind::Array, 0)])
    );
    assert_eq!(tags.type_annotations[0].annotation, type_use("elem"));

    let wild = class.field("wild").expect("Kitchen.wild exists");
    assert_eq!(
        wild.type_annotations[0].path,
        path(&[(TypePathKind::TypeArgument, 0), (TypePathKind::Wildcard, 0)])
    );
    assert_eq!(wild.type_annotations[0].annotation, type_use("wild"));

    let nested = class.field("nested").expect("Kitchen.nested exists");
    assert_eq!(
        nested.type_annotations[0].path,
        path(&[(TypePathKind::Nested, 0)])
    );
    assert_eq!(nested.type_annotations[0].annotation, type_use("nested"));

    let invisible = class
        .field("invisibleType")
        .expect("Kitchen.invisibleType exists");
    assert_eq!(invisible.invisible_type_annotations.len(), 1);
    assert!(invisible.invisible_type_annotations[0].path.is_empty());
    assert_eq!(
        invisible.invisible_type_annotations[0].annotation,
        annotation("Ldemo/HiddenType;", vec![])
    );
}

#[test]
fn fields_keep_declaration_order_with_their_typed_attributes() {
    let class = metadata("Kitchen");
    let names: Vec<&str> = class
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "names",
            "tags",
            "wild",
            "nested",
            "hidden",
            "invisibleType",
            "LIMIT",
        ]
    );

    let names_field = class.field("names").expect("Kitchen.names exists");
    assert_eq!(names_field.descriptor, "Ljava/util/List;");
    assert_eq!(names_field.access_flags, 0x0012);
    assert!(!names_field.is_static());
    assert_eq!(
        names_field.signature.as_deref(),
        Some("Ljava/util/List<Ljava/lang/String;>;")
    );
    assert_eq!(
        names_field.annotations,
        vec![
            annotation("Ldemo/Defaults;", vec![("name", string("field"))]),
            type_use("field"),
        ]
    );
    assert_eq!(names_field.constant_value, None);
    assert!(!names_field.deprecated);
}

#[test]
fn constant_values_are_typed() {
    let class = metadata("Kitchen");
    let limit = class.field("LIMIT").expect("Kitchen.LIMIT exists");
    assert_eq!(limit.access_flags, 0x0018);
    assert_eq!(limit.constant_value, Some(ConstantValue::Integer(4)));
    assert!(limit.annotations.is_empty());
    assert_eq!(limit.type_annotations, vec![]);
}

#[test]
fn methods_keep_class_file_order_with_descriptors() {
    let class = metadata("Kitchen");
    let signatures: Vec<(&str, &str)> = class
        .methods
        .iter()
        .map(|method| (method.name.as_str(), method.descriptor.as_str()))
        .collect();
    assert_eq!(
        signatures,
        vec![
            ("<init>", "(Ljava/lang/String;I)V"),
            (
                "pick",
                "(Ljava/lang/Comparable;Ljava/lang/Comparable;)Ljava/lang/Comparable;"
            ),
            ("io", "()V"),
            ("varargs", "([Ljava/lang/String;)V"),
            ("legacy", "()V"),
            ("make", "()Ljava/lang/Object;"),
            ("compareTo", "(Ldemo/Kitchen;)I"),
            ("compareTo", "(Ljava/lang/Object;)I"),
        ]
    );
    // Constructors are methods named `<init>`; there is no separate node.
    let constructor = class
        .method("<init>", "(Ljava/lang/String;I)V")
        .expect("the canonical constructor exists");
    assert!(constructor.is_constructor());
    assert!(!constructor.is_class_initializer());
    let pick = class
        .method(
            "pick",
            "(Ljava/lang/Comparable;Ljava/lang/Comparable;)Ljava/lang/Comparable;",
        )
        .expect("Kitchen.pick exists");
    assert_eq!(
        pick.signature.as_deref(),
        Some("<T::Ljava/lang/Comparable<TT;>;>(TT;TT;)TT;")
    );
}

#[test]
fn method_parameters_keep_names_and_flags() {
    let class = metadata("Kitchen");
    let constructor = class
        .method("<init>", "(Ljava/lang/String;I)V")
        .expect("the canonical constructor exists");
    assert_eq!(
        constructor.parameters,
        Some(vec![
            MethodParameter {
                name: Some("name".to_owned()),
                access_flags: 0,
            },
            MethodParameter {
                name: Some("count".to_owned()),
                access_flags: 0,
            },
        ])
    );
    assert_eq!(
        constructor.parameter_names(),
        Some(vec![Some("name"), Some("count")])
    );

    let varargs = class
        .method("varargs", "([Ljava/lang/String;)V")
        .expect("Kitchen.varargs exists");
    assert!(varargs.is_varargs());
    assert_eq!(
        varargs.parameters,
        Some(vec![MethodParameter {
            name: Some("rest".to_owned()),
            access_flags: 0,
        }])
    );

    // A compiler-generated bridge method still records a (synthetic) parameter name.
    let bridge = class
        .method("compareTo", "(Ljava/lang/Object;)I")
        .expect("the bridge method exists");
    assert!(bridge.is_bridge());
    assert!(bridge.is_synthetic());
    assert_eq!(
        bridge.parameters,
        Some(vec![MethodParameter {
            name: Some("other".to_owned()),
            access_flags: 0x1000,
        }])
    );

    // An anonymous class constructor takes the captured outer instance, `final mandated`.
    let anonymous = metadata("Kitchen$1");
    let constructor = anonymous
        .method("<init>", "(Ldemo/Kitchen;)V")
        .expect("the anonymous constructor exists");
    assert_eq!(
        constructor.parameters,
        Some(vec![MethodParameter {
            name: Some("this$0".to_owned()),
            access_flags: 0x8010,
        }])
    );
}

#[test]
fn parameter_annotations_keep_parameter_and_declaration_order() {
    let class = metadata("Kitchen");
    let constructor = class
        .method("<init>", "(Ljava/lang/String;I)V")
        .expect("the canonical constructor exists");
    assert_eq!(
        constructor.parameter_annotations,
        Some(vec![
            vec![type_use("ctor")],
            vec![annotation(
                "Ldemo/Defaults;",
                vec![("name", string("param"))]
            )],
        ])
    );
    assert_eq!(constructor.type_annotations.len(), 1);
    assert_eq!(constructor.type_annotations[0].target_type, 0x16);
    assert_eq!(
        constructor.type_annotations[0].target,
        TypeAnnotationTarget::FormalParameter { index: 0 }
    );

    // The visible list has an empty entry for the second parameter; the invisible list is
    // its own attribute, and the model does not merge them.
    let pick = class
        .method(
            "pick",
            "(Ljava/lang/Comparable;Ljava/lang/Comparable;)Ljava/lang/Comparable;",
        )
        .expect("Kitchen.pick exists");
    assert_eq!(
        pick.parameter_annotations,
        Some(vec![vec![type_use("param")], vec![]])
    );
    assert_eq!(
        pick.invisible_parameter_annotations,
        Some(vec![vec![], vec![hidden("right")]])
    );

    let plain = class
        .method("varargs", "([Ljava/lang/String;)V")
        .expect("Kitchen.varargs exists");
    assert_eq!(plain.parameter_annotations, None);
    assert_eq!(plain.invisible_parameter_annotations, None);
}

#[test]
fn exceptions_and_throws_type_annotations_are_typed() {
    let class = metadata("Kitchen");
    let io = class.method("io", "()V").expect("Kitchen.io exists");
    assert_eq!(io.exceptions, vec!["java/io/IOException".to_owned()]);
    assert_eq!(io.type_annotations.len(), 1);
    assert_eq!(io.type_annotations[0].target_type, 0x17);
    assert_eq!(
        io.type_annotations[0].target,
        TypeAnnotationTarget::Throws { index: 0 }
    );
    assert_eq!(io.type_annotations[0].annotation, type_use("throws"));
}

#[test]
fn the_deprecated_attribute_is_a_typed_flag() {
    let class = metadata("Kitchen");
    let legacy = class
        .method("legacy", "()V")
        .expect("Kitchen.legacy exists");
    assert!(legacy.deprecated);
    assert_eq!(
        legacy.annotations,
        vec![annotation("Ljava/lang/Deprecated;", vec![])]
    );
    assert!(
        !class
            .method("io", "()V")
            .expect("Kitchen.io exists")
            .deprecated
    );
}

#[test]
fn nestmates_are_typed_on_both_sides() {
    let class = metadata("Kitchen");
    // Class-file order, deliberately not sorted in the model.
    assert_eq!(
        class.nest_members,
        vec![
            "demo/Kitchen$Inner".to_owned(),
            "demo/Kitchen$Entry".to_owned(),
            "demo/Kitchen$1".to_owned(),
        ]
    );
    for name in ["Kitchen$1", "Kitchen$Entry", "Kitchen$Inner"] {
        let nested = metadata(name);
        assert_eq!(
            nested.nest_host.as_deref(),
            Some("demo/Kitchen"),
            "{name} is a nestmate of Kitchen"
        );
        assert!(nested.nest_members.is_empty(), "{name} has no members");
    }
}

#[test]
fn inner_classes_keep_outer_names_and_flags() {
    let class = metadata("Kitchen");
    assert_eq!(class.inner_classes.len(), 3);

    let inner = class
        .inner_classes
        .iter()
        .find(|entry| entry.inner_class == "demo/Kitchen$Inner")
        .expect("InnerClasses names Kitchen$Inner");
    assert_eq!(inner.outer_class.as_deref(), Some("demo/Kitchen"));
    assert_eq!(inner.inner_name.as_deref(), Some("Inner"));
    assert_eq!(inner.access_flags, 0x0000);

    let anonymous = class
        .inner_classes
        .iter()
        .find(|entry| entry.inner_class == "demo/Kitchen$1")
        .expect("InnerClasses names Kitchen$1");
    assert_eq!(anonymous.outer_class, None);
    assert_eq!(anonymous.inner_name, None);
    assert_eq!(anonymous.access_flags, 0x0000);

    let entry = class
        .inner_classes
        .iter()
        .find(|entry| entry.inner_class == "demo/Kitchen$Entry")
        .expect("InnerClasses names Kitchen$Entry");
    assert_eq!(entry.outer_class.as_deref(), Some("demo/Kitchen"));
    assert_eq!(entry.inner_name.as_deref(), Some("Entry"));
    assert_eq!(entry.access_flags, 0x0009);
}

#[test]
fn an_anonymous_class_records_its_enclosing_method() {
    let anonymous = metadata("Kitchen$1");
    assert_eq!(
        anonymous.enclosing_method,
        Some(EnclosingMethod {
            class: "demo/Kitchen".to_owned(),
            method: Some(MethodNameAndDescriptor {
                name: "make".to_owned(),
                descriptor: "()Ljava/lang/Object;".to_owned(),
            }),
        })
    );
    assert_eq!(metadata("Kitchen").enclosing_method, None);
}

#[test]
fn record_components_are_typed_with_their_annotations() {
    let circle = metadata("Circle");
    assert!(circle.is_record());
    assert!(!circle.is_sealed());
    assert_eq!(circle.super_class.as_deref(), Some("java/lang/Record"));
    let components = circle
        .record_components
        .as_deref()
        .expect("Circle carries a Record attribute");
    assert_eq!(components.len(), 1);

    let radius = circle
        .record_component("radius")
        .expect("the radius component exists");
    assert_eq!(radius.descriptor, "D");
    assert_eq!(radius.signature, None);
    // Inside the `Record` attribute the declaration annotation and the type-use
    // annotation are separate attributes: only `@Defaults` is a component annotation.
    assert_eq!(
        radius.annotations,
        vec![annotation(
            "Ldemo/Defaults;",
            vec![("name", string("component"))]
        )]
    );
    assert_eq!(radius.type_annotations.len(), 1);
    assert_eq!(radius.type_annotations[0].target_type, 0x13);
    assert_eq!(
        radius.type_annotations[0].target,
        TypeAnnotationTarget::Empty
    );
    assert_eq!(radius.type_annotations[0].annotation, type_use("r"));

    // javac propagates a record component's annotations to the field, the accessor and the
    // canonical constructor parameter; the model keeps all four copies because reflection
    // exposes all four. The field and the accessor also carry the type-use annotation as a
    // declaration annotation, which is why they have one more than the component.
    let field_annotations = vec![
        annotation("Ldemo/Defaults;", vec![("name", string("component"))]),
        type_use("r"),
    ];
    assert_eq!(
        circle
            .field("radius")
            .expect("the backing field exists")
            .annotations,
        field_annotations
    );
    let accessor = circle.method("radius", "()D").expect("the accessor exists");
    assert_eq!(accessor.annotations, field_annotations);
    assert_eq!(accessor.type_annotations.len(), 1);
    assert_eq!(
        accessor.type_annotations[0].target,
        TypeAnnotationTarget::Empty
    );
    assert_eq!(accessor.type_annotations[0].target_type, 0x14);
    assert_eq!(
        circle
            .method("<init>", "(D)V")
            .expect("the canonical constructor exists")
            .parameter_annotations,
        Some(vec![field_annotations])
    );
}

#[test]
fn sealed_permitted_subclasses_are_typed() {
    let shapes = metadata("Shapes");
    assert!(shapes.is_interface());
    assert!(shapes.is_sealed());
    assert_eq!(shapes.access_flags, 0x0601);
    assert_eq!(
        shapes.permitted_subclasses,
        Some(vec!["demo/Circle".to_owned(), "demo/Square".to_owned()])
    );
    assert!(!metadata("Circle").is_sealed());
    assert_eq!(metadata("Circle").permitted_subclasses, None);
}

#[test]
fn generic_signatures_are_retained_verbatim() {
    let box_class = metadata("Box");
    assert_eq!(
        box_class.signature.as_deref(),
        Some("<T::Ljava/lang/Comparable<TT;>;:Ljava/io/Serializable;>Ljava/lang/Object;")
    );
    assert_eq!(
        box_class
            .field("value")
            .expect("Box.value exists")
            .signature
            .as_deref(),
        Some("TT;")
    );
    assert_eq!(
        box_class
            .method("<init>", "(Ljava/lang/Comparable;)V")
            .expect("Box.<init> exists")
            .signature
            .as_deref(),
        Some("(TT;)V")
    );
    assert_eq!(
        box_class
            .method("get", "()Ljava/lang/Comparable;")
            .expect("Box.get exists")
            .signature
            .as_deref(),
        Some("()TT;")
    );
    assert_eq!(
        box_class
            .method("map", "(Ljava/util/function/Function;)Ljava/lang/Object;")
            .expect("Box.map exists")
            .signature
            .as_deref(),
        Some("<R:Ljava/lang/Object;>(Ljava/util/function/Function<-TT;+TR;>;)TR;")
    );

    let int_box = metadata("IntBox");
    assert_eq!(
        int_box.signature.as_deref(),
        Some("Ldemo/Box<Ljava/lang/Integer;>;")
    );
    let bridge = int_box
        .method("get", "()Ljava/lang/Comparable;")
        .expect("IntBox has a bridge get()");
    assert!(bridge.is_bridge());
    assert!(bridge.is_synthetic());
}

#[test]
fn annotation_defaults_are_typed_for_every_element_value_kind() {
    let class = metadata("Defaults");
    let expected = vec![
        ("flag", ElementValue::Boolean(true)),
        ("small", ElementValue::Byte(7)),
        ("letter", ElementValue::Char(u16::from(b'r'))),
        ("medium", ElementValue::Short(300)),
        ("count", ElementValue::Int(3)),
        ("big", ElementValue::Long(42)),
        ("ratio32", ElementValue::Float(1.5)),
        ("ratio", ElementValue::Double(2.25)),
        ("name", string("anon")),
        ("type", ElementValue::Class("Ljava/lang/String;".to_owned())),
        (
            "color",
            ElementValue::Enum {
                type_descriptor: "Ldemo/Color;".to_owned(),
                constant: "RED".to_owned(),
            },
        ),
        (
            "nested",
            ElementValue::Annotation(annotation("Ldemo/Nested;", vec![("value", string("d"))])),
        ),
        (
            "primes",
            ElementValue::Array(vec![
                ElementValue::Int(2),
                ElementValue::Int(3),
                ElementValue::Int(5),
            ]),
        ),
        ("aliases", ElementValue::Array(vec![string("x")])),
    ];
    for (name, value) in expected {
        let method = class
            .methods_named(name)
            .next()
            .unwrap_or_else(|| panic!("Defaults.{name} exists"));
        assert_eq!(
            method.annotation_default,
            Some(value),
            "Defaults.{name} AnnotationDefault"
        );
    }

    // An element without a default carries no AnnotationDefault attribute at all.
    let nested = metadata("Nested");
    let value = nested
        .methods_named("value")
        .next()
        .expect("Nested.value exists");
    assert_eq!(value.annotation_default, None);
}

#[test]
fn unmodeled_attributes_are_kept_verbatim_and_reported_by_the_dump() {
    let mut class = parse("Kitchen");
    // Reuse an existing Utf8 as the name of an attribute the model does not know. What is
    // being tested is that the payload survives and the dump names it, not the name.
    let name_index = utf8_index(&class, "kitchen");
    class.fields[0].attributes.push(Attribute {
        name_index,
        data: vec![0xDE, 0xAD],
    });

    let metadata = class
        .metadata()
        .expect("an unknown attribute must not stop decoding");
    let raw = metadata
        .field("names")
        .expect("Kitchen.names exists")
        .attributes
        .iter()
        .find(|attribute| attribute.name == "kitchen")
        .expect("the unknown attribute is retained");
    assert_eq!(raw.data, vec![0xDE, 0xAD]);

    let text = dump(&metadata);
    assert!(
        text.contains("    unknown_attributes 1\n      kitchen\n"),
        "the dump must report the unmodeled attribute:\n{text}"
    );
}

#[test]
fn a_truncated_annotation_payload_is_a_typed_error() {
    let mut class = parse("Kitchen");
    // One annotation, then end of payload: the count promises a payload that is not there.
    replace_class_attribute(&mut class, "RuntimeVisibleAnnotations", vec![0x00, 0x01]);
    let error = class.metadata().expect_err("a truncated payload must fail");
    match error {
        Error::BadAttribute { name, cause } => {
            assert_eq!(name, "RuntimeVisibleAnnotations");
            assert!(
                matches!(*cause, Error::UnexpectedEof { .. }),
                "cause must be the bounds error, got {cause:?}"
            );
        }
        other => panic!("expected BadAttribute, got {other:?}"),
    }
}

#[test]
fn an_annotation_with_a_bad_constant_pool_index_is_a_typed_error() {
    let mut class = parse("Kitchen");
    replace_class_attribute(
        &mut class,
        "RuntimeVisibleAnnotations",
        vec![0x00, 0x01, 0xFF, 0xFF, 0x00, 0x00],
    );
    let error = class.metadata().expect_err("a bad pool index must fail");
    match error {
        Error::BadAttribute { name, cause } => {
            assert_eq!(name, "RuntimeVisibleAnnotations");
            assert!(
                matches!(
                    *cause,
                    Error::BadConstantPoolIndex {
                        index: u16::MAX,
                        ..
                    }
                ),
                "cause must be the pool error, got {cause:?}"
            );
        }
        other => panic!("expected BadAttribute, got {other:?}"),
    }
}

#[test]
fn an_annotation_payload_with_trailing_bytes_is_a_typed_error() {
    let mut class = parse("Kitchen");
    let mut data = class
        .attributes
        .iter()
        .find(|attribute| {
            class.constant_pool.utf8(attribute.name_index).unwrap() == "RuntimeVisibleAnnotations"
        })
        .expect("the fixture has RuntimeVisibleAnnotations")
        .data
        .clone();
    data.push(0x00);
    replace_class_attribute(&mut class, "RuntimeVisibleAnnotations", data);

    let error = class
        .metadata()
        .expect_err("trailing bytes must fail the exact-length check");
    match error {
        Error::BadAttribute { name, cause } => {
            assert_eq!(name, "RuntimeVisibleAnnotations");
            match *cause {
                Error::MalformedAttribute { detail } => {
                    assert!(detail.contains("1 trailing byte"), "detail: {detail}");
                }
                other => panic!("expected MalformedAttribute, got {other:?}"),
            }
        }
        other => panic!("expected BadAttribute, got {other:?}"),
    }
}

#[test]
fn an_unknown_element_value_tag_is_a_typed_error() {
    let mut class = parse("Kitchen");
    let type_index = utf8_index(&class, "Ldemo/Defaults;");
    let name_index = utf8_index(&class, "name");
    let mut data = vec![0x00, 0x01];
    data.extend_from_slice(&type_index.to_be_bytes());
    data.extend_from_slice(&0x0001u16.to_be_bytes());
    data.extend_from_slice(&name_index.to_be_bytes());
    data.push(b'Q'); // not an element_value tag
    data.extend_from_slice(&0x0000u16.to_be_bytes());
    replace_class_attribute(&mut class, "RuntimeVisibleAnnotations", data);

    let error = class.metadata().expect_err("an unknown tag must fail");
    match error {
        Error::BadAttribute { name, cause } => {
            assert_eq!(name, "RuntimeVisibleAnnotations");
            match *cause {
                Error::MalformedAttribute { detail } => {
                    assert!(detail.contains("element_value tag"), "detail: {detail}");
                }
                other => panic!("expected MalformedAttribute, got {other:?}"),
            }
        }
        other => panic!("expected BadAttribute, got {other:?}"),
    }
}

#[test]
fn a_duplicate_decoded_attribute_is_a_typed_error() {
    let mut class = parse("Kitchen");
    let pick = class
        .methods
        .iter()
        .position(|method| class.method_name(method).unwrap() == "pick")
        .expect("Kitchen.pick exists");
    let signature = class.methods[pick]
        .attributes
        .iter()
        .find(|attribute| class.constant_pool.utf8(attribute.name_index).unwrap() == "Signature")
        .expect("Kitchen.pick has a Signature")
        .clone();
    class.methods[pick].attributes.push(signature);

    let error = class
        .metadata()
        .expect_err("JVMS §4.7 allows at most one Signature per method");
    match error {
        Error::BadAttribute { name, cause } => {
            assert_eq!(name, "Signature");
            match *cause {
                Error::MalformedAttribute { detail } => {
                    assert!(detail.contains("duplicate"), "detail: {detail}");
                }
                other => panic!("expected MalformedAttribute, got {other:?}"),
            }
        }
        other => panic!("expected BadAttribute, got {other:?}"),
    }
}

#[test]
fn the_fixtures_cover_every_attribute_the_model_decodes() {
    let mut seen: Vec<String> = Vec::new();
    for name in [
        "Kitchen",
        "Kitchen$1",
        "Kitchen$Entry",
        "Kitchen$Inner",
        "Circle",
        "Shapes",
        "Defaults",
        "Box",
        "IntBox",
        "Nested",
    ] {
        let class = parse(name);
        let mut record = |attribute: &Attribute| {
            let attribute_name = class.attribute_name(attribute).unwrap().to_owned();
            if !seen.contains(&attribute_name) {
                seen.push(attribute_name);
            }
        };
        for attribute in &class.attributes {
            record(attribute);
        }
        for field in &class.fields {
            for attribute in &field.attributes {
                record(attribute);
            }
        }
        for method in &class.methods {
            for attribute in &method.attributes {
                record(attribute);
            }
        }
    }

    for required in [
        "AnnotationDefault",
        "ConstantValue",
        "Deprecated",
        "EnclosingMethod",
        "Exceptions",
        "InnerClasses",
        "MethodParameters",
        "NestHost",
        "NestMembers",
        "PermittedSubclasses",
        "Record",
        "RuntimeInvisibleAnnotations",
        "RuntimeInvisibleParameterAnnotations",
        "RuntimeInvisibleTypeAnnotations",
        "RuntimeVisibleAnnotations",
        "RuntimeVisibleParameterAnnotations",
        "RuntimeVisibleTypeAnnotations",
        "Signature",
        "SourceFile",
    ] {
        assert!(
            seen.iter().any(|name| name == required),
            "no fixture exercises {required}; saw {seen:?}"
        );
    }
}

#[test]
fn the_dump_is_deterministic_and_carries_no_host_state() {
    let first = dump(&metadata("Kitchen"));
    let second = dump(&metadata("Kitchen"));
    assert_eq!(
        first, second,
        "two parses of the same bytes must dump equally"
    );
    // The `ClassFile` convenience entry point must produce the same text.
    assert_eq!(
        parse("Kitchen")
            .metadata_dump()
            .expect("the dump entry point succeeds"),
        first
    );
    assert!(first.ends_with('\n'));
    assert!(!first.contains(env!("CARGO_MANIFEST_DIR")));
    assert!(!first.contains("crates/"));
    assert!(!first.contains("target/"));
    assert!(!first.contains("Last modified"));
}

#[test]
fn golden_dumps_match_the_committed_fixtures() {
    for name in ["Kitchen", "Kitchen$1", "Circle", "Shapes", "Defaults"] {
        let actual = dump(&metadata(name));
        let path = fixture_dir()
            .join("golden")
            .join(format!("{name}.metadata.txt"));
        if std::env::var_os("RAVA_UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all(path.parent().expect("golden directory"))
                .expect("create the golden directory");
            std::fs::write(&path, &actual).expect("write the golden file");
            continue;
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        assert_eq!(
            actual,
            expected,
            "{name} dump differs from {}; the diff is the evidence, so read it before \
             regenerating with RAVA_UPDATE_GOLDEN=1",
            path.display()
        );
    }
}
