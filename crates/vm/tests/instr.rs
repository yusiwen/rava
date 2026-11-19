//! S5 acceptance: the instruction set the host library uses decodes.
//!
//! Two kinds of check live here. The first decodes hand-built byte sequences and asserts the
//! instruction and its operand — the cheap, exact test for a decoder. The second decodes every
//! method body of classes the host library needs for boot, which is the reason S5 exists: the
//! image is decoded at load, so a single unimplemented opcode in `java.lang.String` would make
//! the class unloadable.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use rava_classfile::ClassFile;
use rava_vm::VmError;
use rava_vm::code::{self, Code, Instr};
use rava_vm::image::Image;

/// Wraps instruction bytes in a minimal `Code` attribute payload.
fn decode(bytes: &[u8]) -> Result<Code, VmError> {
    let mut data = Vec::with_capacity(bytes.len() + 12);
    data.extend_from_slice(&8u16.to_be_bytes()); // max_stack
    data.extend_from_slice(&8u16.to_be_bytes()); // max_locals
    data.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    data.extend_from_slice(bytes);
    data.extend_from_slice(&0u16.to_be_bytes()); // exception table
    data.extend_from_slice(&0u16.to_be_bytes()); // attributes
    code::decode(&data, "test.method()V")
}

fn instructions(bytes: &[u8]) -> Vec<Instr> {
    decode(bytes).expect("decodes").instrs
}

#[test]
fn decodes_the_stack_instructions() {
    let cases: &[(u8, Instr)] = &[
        (0x59, Instr::Dup),
        (0x5a, Instr::DupX1),
        (0x5b, Instr::DupX2),
        (0x5c, Instr::Dup2),
        (0x5d, Instr::Dup2X1),
        (0x5e, Instr::Dup2X2),
        (0x5f, Instr::Swap),
    ];
    for (opcode, expected) in cases {
        assert_eq!(
            instructions(&[*opcode, 0xb1]),
            vec![*expected, Instr::Return],
            "opcode 0x{opcode:02X}"
        );
    }
}

#[test]
fn decodes_wide_local_indices() {
    // wide iload 256: the prefix widens the u1 index to u2.
    assert_eq!(
        instructions(&[0xc4, 0x15, 0x01, 0x00, 0xb1]),
        vec![Instr::Iload(256), Instr::Return]
    );
    // wide aload 7.
    assert_eq!(
        instructions(&[0xc4, 0x19, 0x00, 0x07, 0xb1]),
        vec![Instr::Aload(7), Instr::Return]
    );
    // wide astore 300.
    assert_eq!(
        instructions(&[0xc4, 0x3a, 0x01, 0x2c, 0xb1]),
        vec![Instr::Astore(300), Instr::Return]
    );
    // wide lstore 2 (a category-2 store keeps its opcode, only the index widens).
    assert_eq!(
        instructions(&[0xc4, 0x37, 0x00, 0x02, 0xb1]),
        vec![Instr::Lstore(2), Instr::Return]
    );
}

#[test]
fn decodes_wide_iinc_with_a_signed_increment() {
    // wide iinc slot 0x0102 by -2.
    assert_eq!(
        instructions(&[0xc4, 0x84, 0x01, 0x02, 0xff, 0xfe, 0xb1]),
        vec![
            Instr::Iinc {
                slot: 0x0102,
                increment: -2
            },
            Instr::Return
        ]
    );
}

#[test]
fn rejects_a_wide_prefix_around_an_instruction_it_cannot_widen() {
    // `wide ret` exists in the format but `ret` is not part of the set this VM runs.
    match decode(&[0xc4, 0xa9, 0x00, 0x01, 0xb1]) {
        Err(VmError::UnsupportedInstruction { opcode, .. }) => assert_eq!(opcode, 0xa9),
        other => panic!("`wide ret` decoded as {other:?}"),
    }
}

#[test]
fn decodes_goto_w_in_both_directions() {
    // Forward: goto_w at pc 0 with offset 5 lands on the `return` at pc 5.
    assert_eq!(
        instructions(&[0xc8, 0x00, 0x00, 0x00, 0x05, 0xb1]),
        vec![Instr::Goto(1), Instr::Return]
    );
    // Backward: goto_w at pc 1 with offset -1 lands on the `return` at pc 0.
    assert_eq!(
        instructions(&[0xb1, 0xc8, 0xff, 0xff, 0xff, 0xff]),
        vec![Instr::Return, Instr::Goto(0)]
    );
}

/// The devShell exports this; a machine without it is not the machine this acceptance covers.
fn jdk_home() -> PathBuf {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    PathBuf::from(home)
}

fn image() -> &'static Arc<Image> {
    static IMAGE: OnceLock<Arc<Image>> = OnceLock::new();
    IMAGE.get_or_init(|| {
        let path = jdk_home().join("lib/modules");
        Arc::new(Image::open(&path).unwrap_or_else(|error| {
            panic!("cannot read the host image {}: {error}", path.display())
        }))
    })
}

/// Decodes every method body of `binary_name` and returns how many it decoded.
fn decode_class(binary_name: &str) -> usize {
    let bytes = image()
        .class(binary_name)
        .unwrap_or_else(|error| panic!("cannot read {binary_name}: {error}"))
        .unwrap_or_else(|| panic!("{binary_name} is not in the image"));
    let file = ClassFile::parse(bytes)
        .unwrap_or_else(|error| panic!("cannot parse {binary_name}: {error}"));
    let mut decoded = 0;
    for method in &file.methods {
        let name = file.method_name(method).expect("method name");
        let descriptor = file.method_descriptor(method).expect("method descriptor");
        let qualified = format!("{binary_name}.{name}{descriptor}");
        let Some(attribute) = file.code(method).expect("code attribute") else {
            continue;
        };
        code::decode(&attribute.data, &qualified)
            .unwrap_or_else(|error| panic!("{qualified} does not decode: {error}"));
        decoded += 1;
    }
    decoded
}

#[test]
fn host_classes_the_boot_milestone_needs_decode_completely() {
    // The four classes the measurement named: `String.join` needed `dup2`, `Thread`,
    // `ArrayList` and `HashMap` needed `dup_x1`/`dup_x2`.
    let mut total = 0;
    for class in [
        "java/lang/String",
        "java/lang/Thread",
        "java/util/ArrayList",
        "java/util/HashMap",
    ] {
        let decoded = decode_class(class);
        assert!(decoded > 20, "{class} decoded only {decoded} method bodies");
        total += decoded;
    }
    assert!(total > 300, "decoded {total} method bodies in total");
}
