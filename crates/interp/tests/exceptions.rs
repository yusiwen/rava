//! M2.7 acceptance: exceptions, `athrow` and exception tables.
//!
//! `Boom` is an ordinary class extending `Object` whose constructor chains to
//! `Object.<init>` and whose static `thrower()` throws a fresh instance. `Catcher`
//! exercises the interpreter's exception table: a direct catch, a catch-all, a
//! superclass catch, a wrong catch type, an uncaught throw and a throw that crosses two
//! calls before the outermost frame catches it. The malformed-table methods exist only
//! to be decoded, never executed.
//!
//! Every handler entry is named with the assembler's labels and read back through
//! `Assembler::offset`, so the byte offsets in the class file come from the same bytes
//! the decoder sees rather than from hand counting.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder, ClassFile, Label};
use rava_interp::{ExceptionHandler, Instr, InterpError, decode, run_static_method};
use rava_vm::{ClassId, LoaderId, Shared, ThreadState, Value, instance};

const ACC_PUBLIC: u16 = 0x0001;
const ACC_PUBLIC_STATIC: u16 = 0x0009;
/// `invokestatic`, through the raw-opcode helper: the assembler has no named form.
const INVOKESTATIC: u8 = 0xb8;

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

/// `Boom extends Object`, with a constructor that calls `Object.<init>` and a static
/// `thrower()` that throws a fresh instance.
fn boom_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("Boom", "java/lang/Object", 61);
    let object_init = builder.method_ref("java/lang/Object", "<init>", "()V");
    let boom_init = builder.method_ref("Boom", "<init>", "()V");
    let boom_class = builder.class("Boom");

    // Boom() { super(); }
    let mut init = Assembler::new();
    init.aload(0);
    init.invokespecial(object_init);
    init.return_();
    builder.method(
        ACC_PUBLIC,
        "<init>",
        "()V",
        Some((1, 1, init.finish().unwrap())),
    );

    // static void thrower() { throw new Boom(); }
    let mut thrower = Assembler::new();
    thrower.new_instance(boom_class);
    thrower.dup();
    thrower.invokespecial(boom_init);
    thrower.athrow();
    builder.method(
        ACC_PUBLIC_STATIC,
        "thrower",
        "()V",
        Some((2, 0, thrower.finish().unwrap())),
    );

    builder.build()
}

/// The offset a placed label names, as an exception-table `u16`.
fn offset(assembler: &Assembler, label: Label) -> u16 {
    u16::try_from(
        assembler
            .offset(label)
            .expect("the label is placed before the table is built"),
    )
    .expect("the fixture's code fits in a u16")
}

/// `static int <name>() { throw new Boom(); }` with one handler covering the throw and
/// returning `value`.
fn catching_method(
    builder: &mut ClassBuilder,
    name: &str,
    catch_type: u16,
    value: i32,
    boom_class: u16,
    boom_init: u16,
) {
    let mut body = Assembler::new();
    let protected = body.label();
    let protected_end = body.label();
    let handler = body.label();

    body.place(protected);
    body.new_instance(boom_class);
    body.dup();
    body.invokespecial(boom_init);
    body.athrow();
    body.place(protected_end);
    body.place(handler);
    body.iconst(value);
    body.ireturn();

    let start = offset(&body, protected);
    let end = offset(&body, protected_end);
    let handler_pc = offset(&body, handler);
    builder.method_with_handlers(
        ACC_PUBLIC_STATIC,
        name,
        "()I",
        Some((2, 0, body.finish().unwrap())),
        vec![(start, end, handler_pc, catch_type)],
    );
}

fn catcher_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("Catcher", "java/lang/Object", 61);
    let boom_class = builder.class("Boom");
    let boom_init = builder.method_ref("Boom", "<init>", "()V");
    let boom_thrower = builder.method_ref("Boom", "thrower", "()V");
    let middle_ref = builder.method_ref("Catcher", "middle", "()V");
    let object_class = builder.class("java/lang/Object");
    let string_class = builder.class("java/lang/String");

    // Caught as its own class: returns 5.
    catching_method(&mut builder, "test", boom_class, 5, boom_class, boom_init);
    // A catch-all (`catch_type = 0`) matches the same throw.
    catching_method(&mut builder, "catchAll", 0, 9, boom_class, boom_init);
    // `Boom extends Object`, so a superclass catch type matches.
    catching_method(
        &mut builder,
        "catchObject",
        object_class,
        4,
        boom_class,
        boom_init,
    );
    // `String` is not a superclass of `Boom`, so this handler must not run.
    catching_method(
        &mut builder,
        "wrongCatch",
        string_class,
        3,
        boom_class,
        boom_init,
    );

    // static int uncaught() { Boom.thrower(); return 0; }
    // No handler entry, so the throw leaves the frame.
    let mut uncaught = Assembler::new();
    uncaught.op(INVOKESTATIC, &boom_thrower.to_be_bytes());
    uncaught.iconst(0);
    uncaught.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "uncaught",
        "()I",
        Some((1, 0, uncaught.finish().unwrap())),
    );

    // static void middle() { Boom.thrower(); }
    // It has no handler of its own; the throwable crosses this frame unchanged.
    let mut middle = Assembler::new();
    middle.op(INVOKESTATIC, &boom_thrower.to_be_bytes());
    middle.return_();
    builder.method(
        ACC_PUBLIC_STATIC,
        "middle",
        "()V",
        Some((0, 0, middle.finish().unwrap())),
    );

    // static int outer() { middle(); return 0; }
    // The handler at the end catches a throwable raised two frames down and returns 7.
    let mut outer = Assembler::new();
    let outer_protected = outer.label();
    let outer_end = outer.label();
    let outer_handler = outer.label();
    outer.place(outer_protected);
    outer.op(INVOKESTATIC, &middle_ref.to_be_bytes());
    outer.iconst(0);
    outer.ireturn();
    outer.place(outer_end);
    outer.place(outer_handler);
    outer.iconst(7);
    outer.ireturn();
    let start = offset(&outer, outer_protected);
    let end = offset(&outer, outer_end);
    let handler_pc = offset(&outer, outer_handler);
    builder.method_with_handlers(
        ACC_PUBLIC_STATIC,
        "outer",
        "()I",
        Some((2, 0, outer.finish().unwrap())),
        vec![(start, end, handler_pc, boom_class)],
    );

    // static int nullThrow() { throw null; }
    // The catch-all must not see a null handle.
    let mut null_throw = Assembler::new();
    let null_protected = null_throw.label();
    let null_end = null_throw.label();
    let null_handler = null_throw.label();
    null_throw.place(null_protected);
    null_throw.aconst_null();
    null_throw.athrow();
    null_throw.place(null_end);
    null_throw.place(null_handler);
    null_throw.iconst(1);
    null_throw.ireturn();
    let start = offset(&null_throw, null_protected);
    let end = offset(&null_throw, null_end);
    let handler_pc = offset(&null_throw, null_handler);
    builder.method_with_handlers(
        ACC_PUBLIC_STATIC,
        "nullThrow",
        "()I",
        Some((2, 0, null_throw.finish().unwrap())),
        vec![(start, end, handler_pc, 0)],
    );

    // static int nonRefThrow() { throw 1; }
    let mut non_ref = Assembler::new();
    non_ref.iconst(1);
    non_ref.athrow();
    builder.method(
        ACC_PUBLIC_STATIC,
        "nonRefThrow",
        "()I",
        Some((1, 0, non_ref.finish().unwrap())),
    );

    builder.build()
}

/// The malformed handler tables `a_malformed_handler_table_is_a_decode_error` decodes
/// directly.
///
/// Definition-time verification rejects an invalid exception table before a class enters
/// the table, so these methods cannot live in `Catcher`.
fn malformed_handler_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("BadHandlerTable", "java/lang/Object", 61);

    // iconst_1; bipush 1; ireturn, with `start_pc = 2`, which is inside `bipush`.
    builder.method_with_handlers(
        ACC_PUBLIC_STATIC,
        "badHandler",
        "()I",
        Some((1, 0, vec![0x04, 0x10, 0x01, 0xac])),
        vec![(2, 4, 0, 0)],
    );

    // iconst_1; ireturn, with `end_pc` past the end of the code array.
    builder.method_with_handlers(
        ACC_PUBLIC_STATIC,
        "badHandlerEnd",
        "()I",
        Some((1, 0, vec![0x04, 0xac])),
        vec![(0, 5, 0, 0)],
    );

    // `catch_type` names a `CONSTANT_Utf8`, not a `CONSTANT_Class`.
    let not_a_class = builder.utf8("Boom");
    builder.method_with_handlers(
        ACC_PUBLIC_STATIC,
        "badCatchType",
        "()I",
        Some((1, 0, vec![0x04, 0xac])),
        vec![(0, 2, 0, not_a_class)],
    );

    builder.build()
}

struct Fixture {
    shared: Shared,
    boom: ClassId,
    catcher: ClassId,
    thread: ThreadState,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    instance::register_natives(&shared);
    let boom = shared
        .define_class(LoaderId::BOOTSTRAP, "Boom", &boom_bytes())
        .expect("Boom defines");
    let catcher = shared
        .define_class(LoaderId::BOOTSTRAP, "Catcher", &catcher_bytes())
        .expect("Catcher defines");
    let thread = shared.thread_state(1);
    Fixture {
        shared,
        boom,
        catcher,
        thread,
    }
}

fn call(fixture: &Fixture, method: &str) -> Result<Option<Value>, InterpError> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.catcher,
        method,
        "()I",
        Vec::new(),
    )
}

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int result, found {other:?}"),
    }
}

#[test]
fn a_caught_throwable_jumps_to_the_handler_and_returns_5() {
    let fixture = fixture();
    assert_eq!(int_value(call(&fixture, "test").unwrap()), 5);
    assert_eq!(
        fixture.thread.frame_depth(),
        0,
        "the handler returns from the same frame, which is popped"
    );
    assert!(fixture.thread.frame_roots().is_empty());
}

#[test]
fn the_decoded_table_names_instruction_indices_not_byte_offsets() {
    let bytes = catcher_bytes();
    let file = ClassFile::parse(&bytes).expect("Catcher parses");
    let method = file
        .find_method("test", "()I")
        .expect("constant pool resolves")
        .expect("test exists");
    let code = file
        .code(method)
        .expect("Code resolves")
        .expect("test has Code");
    let decoded = decode(&file, &code.data).expect("test decodes");

    // new(0), dup(1), invokespecial(2), athrow(3), handler: iconst_5(4), ireturn(5).
    // The byte range 0..8 therefore becomes the instruction range 0..4.
    assert_eq!(
        decoded.handlers,
        vec![ExceptionHandler {
            start: 0,
            end: 4,
            handler: 4,
            catch_type: Some("Boom".to_owned()),
        }]
    );
    assert_eq!(decoded.instructions[3], Instr::Athrow);
}

#[test]
fn a_nested_callee_throwable_is_caught_by_the_outer_frame() {
    let fixture = fixture();
    assert_eq!(int_value(call(&fixture, "outer").unwrap()), 7);

    // `middle` itself has no handler: called directly, it lets the same throwable out.
    let error = run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.catcher,
        "middle",
        "()V",
        Vec::new(),
    )
    .expect_err("middle does not catch");
    let InterpError::Thrown(throwable) = error else {
        panic!("expected a thrown Boom from middle, found {error:?}");
    };
    assert_eq!(
        fixture.shared.heap().instance(throwable).unwrap().class,
        fixture.boom
    );
    assert_eq!(fixture.thread.frame_depth(), 0);
}

#[test]
fn an_uncaught_throwable_carries_the_object_to_the_caller() {
    let fixture = fixture();
    let error = call(&fixture, "uncaught").expect_err("uncaught lets the throw out");
    let InterpError::Thrown(throwable) = error else {
        panic!("expected a thrown Boom, found {error:?}");
    };
    let heap = fixture.shared.heap();
    let instance = heap
        .instance(throwable)
        .expect("the throwable is still a live instance");
    assert_eq!(instance.class, fixture.boom);
    drop(heap);

    assert_eq!(fixture.thread.frame_depth(), 0);
    assert!(fixture.thread.frame_roots().is_empty());
}

#[test]
fn a_catch_type_that_is_not_a_superclass_does_not_catch() {
    let fixture = fixture();
    let error = call(&fixture, "wrongCatch").expect_err("String does not catch Boom");
    assert!(
        matches!(error, InterpError::Thrown(_)),
        "the throwable must be re-raised unchanged, found {error:?}"
    );
}

#[test]
fn a_catch_all_entry_matches_any_throwable() {
    let fixture = fixture();
    assert_eq!(int_value(call(&fixture, "catchAll").unwrap()), 9);
}

#[test]
fn a_superclass_catch_type_matches_through_the_chain() {
    let fixture = fixture();
    assert_eq!(int_value(call(&fixture, "catchObject").unwrap()), 4);
}

#[test]
fn athrow_of_null_and_of_a_non_reference_are_typed_errors() {
    let fixture = fixture();
    let error = call(&fixture, "nullThrow").expect_err("null is not throwable");
    assert!(matches!(error, InterpError::NullThrow), "found {error:?}");

    let error = call(&fixture, "nonRefThrow").expect_err("an int is not throwable");
    assert!(
        matches!(error, InterpError::NonReferenceThrow),
        "found {error:?}"
    );
}

#[test]
fn a_malformed_handler_table_is_a_decode_error() {
    let bytes = malformed_handler_bytes();
    let file = ClassFile::parse(&bytes).expect("the fixture parses");

    for (name, expected) in [
        ("badHandler", "start_pc"),
        ("badHandlerEnd", "end_pc"),
        ("badCatchType", "not a Class"),
    ] {
        let method = file
            .find_method(name, "()I")
            .expect("constant pool resolves")
            .unwrap_or_else(|| panic!("{name} exists"));
        let code = file
            .code(method)
            .expect("Code resolves")
            .expect("Code exists");
        match decode(&file, &code.data) {
            Err(InterpError::Decode(detail)) => {
                assert!(
                    detail.contains(expected),
                    "{name}: expected {expected:?} in {detail:?}"
                );
            }
            other => panic!("{name}: expected a decode error, found {other:?}"),
        }
    }
}
