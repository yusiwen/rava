//! M0.7 acceptance: verifier dataflow skeleton and deterministic metadata dump.
//!
//! The class fixtures are assembled in-process, so the tests need no JDK and no
//! hand-counted branch offsets. The metadata fixture is a committed class file from
//! `rava-classfile`; its dump is committed beside the VM tests.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder, ClassFile};
use rava_vm::{VerifyError, dump_class, verify_method};

const ACC_PUBLIC_STATIC: u16 = 0x0009;

fn assemble(build: impl FnOnce(&mut Assembler)) -> Vec<u8> {
    let mut assembler = Assembler::new();
    build(&mut assembler);
    assembler.finish().expect("fixture assembles")
}

fn case_with_handlers(
    descriptor: &str,
    code: (u16, u16, Vec<u8>),
    handlers: Vec<(u16, u16, u16, u16)>,
) -> ClassFile {
    let mut builder = ClassBuilder::new("VerifierCase", "java/lang/Object", 52);
    builder.method_with_handlers(ACC_PUBLIC_STATIC, "run", descriptor, Some(code), handlers);
    ClassFile::parse(&builder.build()).expect("fixture class parses")
}

fn case(descriptor: &str, code: (u16, u16, Vec<u8>)) -> ClassFile {
    case_with_handlers(descriptor, code, Vec::new())
}

fn run_method<'a>(class: &'a ClassFile, descriptor: &str) -> &'a rava_classfile::Method {
    class
        .find_method("run", descriptor)
        .expect("constant pool resolves")
        .expect("run method exists")
}

#[test]
fn accepts_a_well_formed_method() {
    let code = assemble(|assembler| {
        assembler.iconst(1);
        assembler.istore(0);
        assembler.iload(0);
        assembler.iconst(2);
        assembler.iadd();
        assembler.ireturn();
    });
    let class = case("()I", (2, 1, code));

    let verified = verify_method(&class, run_method(&class, "()I")).expect("method verifies");
    assert_eq!(verified.max_stack, 2);
    assert_eq!(verified.max_locals, 1);
    assert_eq!(verified.code_len, 8);
    assert_eq!(
        verified.frames.len(),
        6,
        "every straight-line instruction boundary is reached"
    );
    assert_eq!(
        verified.frame_at(3).expect("load boundary").locals,
        [0].into_iter().collect()
    );
    assert_eq!(verified.frame_at(7).expect("return boundary").stack, 1);
}

#[test]
fn rejects_stack_underflow() {
    let code = assemble(|assembler| {
        assembler.iconst(1);
        assembler.iadd();
        assembler.ireturn();
    });
    let class = case("()I", (2, 0, code));

    let error = verify_method(&class, run_method(&class, "()I")).unwrap_err();
    assert_eq!(
        error,
        VerifyError::StackUnderflow {
            pc: 1,
            needed: 2,
            found: 1,
        }
    );
}

#[test]
fn rejects_max_stack_overflow() {
    let code = assemble(|assembler| {
        assembler.iconst(1);
        assembler.iconst(2);
        assembler.ireturn();
    });
    let class = case("()I", (1, 0, code));

    let error = verify_method(&class, run_method(&class, "()I")).unwrap_err();
    assert_eq!(
        error,
        VerifyError::StackOverflow {
            pc: 1,
            max_stack: 1,
            found: 2,
        }
    );
}

#[test]
fn rejects_an_invalid_local_index() {
    let code = assemble(|assembler| {
        assembler.iload(2);
        assembler.ireturn();
    });
    let class = case("()I", (2, 1, code));

    let error = verify_method(&class, run_method(&class, "()I")).unwrap_err();
    assert_eq!(
        error,
        VerifyError::InvalidLocal {
            pc: 0,
            index: 2,
            max_locals: 1,
        }
    );
}

#[test]
fn rejects_a_load_from_an_uninitialized_local() {
    let code = assemble(|assembler| {
        assembler.iload(0);
        assembler.ireturn();
    });
    let class = case("()I", (1, 1, code));

    let error = verify_method(&class, run_method(&class, "()I")).unwrap_err();
    assert_eq!(error, VerifyError::UninitializedLocal { pc: 0, index: 0 });
}

#[test]
fn rejects_a_truncated_operand() {
    let class = case("()V", (1, 0, vec![0x10]));

    let error = verify_method(&class, run_method(&class, "()V")).unwrap_err();
    assert_eq!(
        error,
        VerifyError::TruncatedOperand {
            pc: 0,
            opcode: 0x10,
            needed: 1,
            available: 0,
        }
    );
}

#[test]
fn reports_an_unsupported_opcode_instead_of_passing_it() {
    // `ret` (`0xa9`) is not part of the verifier's covered set; the switches and `wide`
    // used to stand in for it but are decoded now.
    let class = case("()V", (1, 0, vec![0xa9]));

    let error = verify_method(&class, run_method(&class, "()V")).unwrap_err();
    assert_eq!(
        error,
        VerifyError::UnsupportedOpcode {
            pc: 0,
            opcode: 0xa9,
        }
    );
}

#[test]
fn verifies_tableswitch_and_lookupswitch_targets() {
    let table = assemble(|assembler| {
        let one = assembler.label();
        let otherwise = assembler.label();
        assembler.iload(0);
        assembler.tableswitch(otherwise, 1, &[one]);
        assembler.place(one);
        assembler.iconst(10);
        assembler.ireturn();
        assembler.place(otherwise);
        assembler.iconst(0);
        assembler.ireturn();
    });
    let class = case("(I)I", (1, 1, table));
    let verified = verify_method(&class, run_method(&class, "(I)I")).expect("tableswitch verifies");
    assert_eq!(
        verified.frames.len(),
        6,
        "the entry, the switch and each arm's constant/return boundaries are reachable"
    );

    let lookup = assemble(|assembler| {
        let first = assembler.label();
        let hundred = assembler.label();
        let otherwise = assembler.label();
        assembler.iload(0);
        assembler.lookupswitch(otherwise, &[(1, first), (100, hundred)]);
        assembler.place(first);
        assembler.iconst(1);
        assembler.ireturn();
        assembler.place(hundred);
        assembler.iconst(100);
        assembler.ireturn();
        assembler.place(otherwise);
        assembler.iconst(0);
        assembler.ireturn();
    });
    let class = case("(I)I", (1, 1, lookup));
    let verified =
        verify_method(&class, run_method(&class, "(I)I")).expect("lookupswitch verifies");
    assert_eq!(
        verified.frames.len(),
        8,
        "the entry, the switch and each of the three arms are reachable"
    );
}

#[test]
fn rejects_switch_targets_that_are_not_instruction_boundaries() {
    // `iconst_0` supplies the selector; the `tableswitch` at pc 1 then targets byte 19,
    // the last byte of the case offset rather than the `return` at byte 20.
    let mut operand = vec![0u8; 2];
    operand.extend_from_slice(&18i32.to_be_bytes()); // default: pc 1 + 18 = 19
    operand.extend_from_slice(&0i32.to_be_bytes());
    operand.extend_from_slice(&0i32.to_be_bytes());
    operand.extend_from_slice(&20i32.to_be_bytes()); // case: pc 1 + 19 = 20
    let code = assemble(|assembler| {
        assembler.iconst(0);
        assembler.op(0xaa, &operand);
        assembler.return_();
    });
    let class = case("(I)I", (1, 1, code));

    let error = verify_method(&class, run_method(&class, "(I)I")).unwrap_err();
    assert_eq!(
        error,
        VerifyError::InvalidBranchTarget {
            pc: 1,
            target: 19,
            code_len: 21,
        }
    );
}

#[test]
fn rejects_a_lookupswitch_with_unsorted_keys() {
    let mut operand = vec![0u8; 3];
    operand.extend_from_slice(&0i32.to_be_bytes()); // default
    operand.extend_from_slice(&2i32.to_be_bytes()); // two pairs
    operand.extend_from_slice(&20i32.to_be_bytes());
    operand.extend_from_slice(&0i32.to_be_bytes());
    operand.extend_from_slice(&10i32.to_be_bytes());
    operand.extend_from_slice(&0i32.to_be_bytes());
    let code = assemble(|assembler| {
        assembler.op(0xab, &operand);
        assembler.return_();
    });
    let class = case("(I)I", (1, 1, code));

    let error = verify_method(&class, run_method(&class, "(I)I")).unwrap_err();
    assert!(
        matches!(error, VerifyError::MalformedCode { ref detail } if detail.contains("strictly ascending")),
        "found {error:?}"
    );
}

#[test]
fn verifies_wide_local_access_and_rejects_an_out_of_range_index() {
    let code = assemble(|assembler| {
        assembler.iconst(7);
        assembler.op(0xc4, &[0x36, 0x01, 0x00]); // wide istore 256
        assembler.op(0xc4, &[0x15, 0x01, 0x00]); // wide iload 256
        assembler.ireturn();
    });
    let class = case("()I", (1, 257, code));
    let verified = verify_method(&class, run_method(&class, "()I")).expect("wide verifies");
    assert_eq!(verified.max_locals, 257);

    // `wide iload 1` with max_locals 1 is out of range.
    let code = assemble(|assembler| {
        assembler.op(0xc4, &[0x15, 0x00, 0x01]);
        assembler.ireturn();
    });
    let class = case("()I", (1, 1, code));
    let error = verify_method(&class, run_method(&class, "()I")).unwrap_err();
    assert_eq!(
        error,
        VerifyError::InvalidLocal {
            pc: 0,
            index: 1,
            max_locals: 1,
        }
    );
}

#[test]
fn rejects_wide_ret() {
    // `wide ret 1`: the modified opcode is `ret` (`0xa9`), which stays unsupported.
    let class = case("()V", (1, 1, vec![0xc4, 0xa9, 0x00, 0x01]));

    let error = verify_method(&class, run_method(&class, "()V")).unwrap_err();
    assert_eq!(
        error,
        VerifyError::UnsupportedOpcode {
            pc: 0,
            opcode: 0xa9,
        }
    );
}

#[test]
fn follows_both_sides_of_a_branch_and_merges_at_the_join() {
    let code = assemble(|assembler| {
        let otherwise = assembler.label();
        let join = assembler.label();
        assembler.iconst(1);
        assembler.if_cond(0x99, otherwise); // ifeq
        assembler.iconst(2);
        assembler.goto(join);
        assembler.place(otherwise);
        assembler.iconst(3);
        assembler.place(join);
        assembler.ireturn();
    });
    let class = case("()I", (1, 0, code));

    let verified = verify_method(&class, run_method(&class, "()I")).expect("both arms verify");
    assert_eq!(
        verified.frames.len(),
        6,
        "the worklist must reach both arms and the join"
    );
    let join_pc = *verified.frames.keys().next_back().expect("join frame");
    assert_eq!(
        verified.frame_at(join_pc).expect("join frame").stack,
        1,
        "both arms leave one value on the stack at the join"
    );
    assert_eq!(
        verified
            .frames
            .values()
            .filter(|frame| frame.stack == 0)
            .count(),
        3,
        "the entry and each arm have one empty-stack instruction boundary"
    );
}

#[test]
fn enters_an_exception_handler_with_the_exception_on_the_stack() {
    // iconst_1; istore_0; return; handler: astore_1; return
    let class = case_with_handlers(
        "()V",
        (1, 2, vec![0x04, 0x36, 0x00, 0xb1, 0x4c, 0xb1]),
        vec![(1, 3, 4, 0)],
    );

    let verified = verify_method(&class, run_method(&class, "()V")).expect("handler verifies");
    assert_eq!(verified.exception_table.len(), 1);
    let handler = verified.frame_at(4).expect("handler frame");
    assert_eq!(handler.stack, 1);
    assert!(
        handler.locals.contains(&0),
        "local 0 is assigned before the protected range"
    );
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn verifies_the_hello_fixture() {
    let bytes = std::fs::read(manifest_dir().join("../classfile/tests/fixtures/Hello.class"))
        .expect("Hello.class fixture is readable");
    let class = ClassFile::parse(&bytes).expect("Hello.class parses");

    for method in &class.methods {
        let name = class.method_name(method).expect("method name resolves");
        verify_method(&class, method)
            .unwrap_or_else(|error| panic!("Hello.{name} does not verify: {error}"));
    }
}

#[test]
fn metadata_dump_matches_the_committed_fixture() {
    let bytes = std::fs::read(manifest_dir().join("../classfile/tests/fixtures/Hello.class"))
        .expect("Hello.class fixture is readable");
    let class = ClassFile::parse(&bytes).expect("Hello.class parses");
    let dump = dump_class(&class).expect("metadata dump succeeds");
    let expected =
        std::fs::read_to_string(manifest_dir().join("tests/fixtures/Hello.metadata.txt"))
            .expect("committed dump is readable");

    assert_eq!(dump, expected);
}

#[test]
fn metadata_dump_lists_annotations_signature_record_and_nest_attributes() {
    // A minimal class whose top-level attributes are the metadata kinds M0 must retain.
    // The parser accepts arbitrary attribute payloads by design; the dump only needs the
    // name and the raw length.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE]);
    bytes.extend_from_slice(&0u16.to_be_bytes()); // minor
    bytes.extend_from_slice(&52u16.to_be_bytes()); // major
    bytes.extend_from_slice(&9u16.to_be_bytes()); // constant pool count

    fn utf8_entry(text: &str) -> Vec<u8> {
        let mut entry = vec![1u8];
        entry.extend_from_slice(&(text.len() as u16).to_be_bytes());
        entry.extend_from_slice(text.as_bytes());
        entry
    }
    fn class_entry(name_index: u16) -> Vec<u8> {
        let mut entry = vec![7u8];
        entry.extend_from_slice(&name_index.to_be_bytes());
        entry
    }

    bytes.extend_from_slice(&utf8_entry("Meta")); // 1
    bytes.extend_from_slice(&class_entry(1)); // 2
    bytes.extend_from_slice(&utf8_entry("java/lang/Object")); // 3
    bytes.extend_from_slice(&class_entry(3)); // 4
    for name in [
        "RuntimeVisibleAnnotations",
        "Signature",
        "Record",
        "NestHost",
    ] {
        bytes.extend_from_slice(&utf8_entry(name)); // 5..=8
    }
    bytes.extend_from_slice(&0x0021u16.to_be_bytes()); // public super
    bytes.extend_from_slice(&2u16.to_be_bytes()); // this_class
    bytes.extend_from_slice(&4u16.to_be_bytes()); // super_class
    bytes.extend_from_slice(&0u16.to_be_bytes()); // interfaces
    bytes.extend_from_slice(&0u16.to_be_bytes()); // fields
    bytes.extend_from_slice(&0u16.to_be_bytes()); // methods
    bytes.extend_from_slice(&4u16.to_be_bytes()); // attributes
    for name_index in 5..=8u16 {
        bytes.extend_from_slice(&name_index.to_be_bytes());
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&[0x00, 0x00]);
    }

    let class = ClassFile::parse(&bytes).expect("metadata fixture parses");
    let dump = dump_class(&class).expect("metadata dump succeeds");
    assert_eq!(
        dump,
        "class Meta\n\
         version 52.0\n\
         access_flags 0x0021\n\
         super java/lang/Object\n\
         interfaces 0\n\
         fields 0\n\
         methods 0\n\
         attributes 4\n\
         \x20 NestHost length 2\n\
         \x20 Record length 2\n\
         \x20 RuntimeVisibleAnnotations length 2\n\
         \x20 Signature length 2\n"
    );
}
