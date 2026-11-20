//! S14 acceptance: `System.arraycopy` moves primitive and reference elements.
//!
//! `arraycopy` is the primitive `Arrays.copyOf`, `String.concat`, `String.substring` and the
//! library's own growth paths are built on, so it has to work before the rest of `java.lang.String`
//! can. This target drives real bytecode: the fixtures allocate `newarray`/`anewarray`, fill,
//! copy, and either read the result back through static fields or fail with the exception the
//! specification names.
//!
//! The cases are the contract's three refusals (null, kind, bounds) and the success cases that
//! are easy to get subtly wrong: a primitive range, a reference range (identity must survive,
//! not just text), and an overlapping copy in both directions.

use std::path::{Path, PathBuf};

use rava_classfile::ClassBuilder;
use rava_vm::{ClassPath, ObjRef, Shared, ThreadState, Value, VmError};

// `newarray` atype codes (JVMS §6.5 `newarray`).
const T_BYTE: u8 = 8;
const T_INT: u8 = 10;

const ALOAD: u8 = 0x19;
const ASTORE: u8 = 0x3a;
const ACONST_NULL: u8 = 0x01;
const LDC_W: u8 = 0x13;
const IALOAD: u8 = 0x2e;
const AALOAD: u8 = 0x32;
const BALOAD: u8 = 0x33;
const IASTORE: u8 = 0x4f;
const AASTORE: u8 = 0x53;
const BASTORE: u8 = 0x54;
const PUTSTATIC: u8 = 0xb3;
const INVOKESTATIC: u8 = 0xb8;
const RETURN: u8 = 0xb1;
const NEWARRAY: u8 = 0xbc;
const ANEWARRAY: u8 = 0xbd;

const ARRAYCOPY: &str = "(Ljava/lang/Object;ILjava/lang/Object;II)V";

/// A tiny bytecode emitter. `ClassBuilder` writes class files, not instructions; a fixture says
/// what it wants and this owns the operand encodings, which is where S10's hand-assembled
/// `if_acmpne` offset went wrong.
#[derive(Default)]
struct Code(Vec<u8>);

impl Code {
    fn op(&mut self, opcode: u8) {
        self.0.push(opcode);
    }

    fn op_u16(&mut self, opcode: u8, operand: u16) {
        self.0
            .extend_from_slice(&[opcode, (operand >> 8) as u8, operand as u8]);
    }

    /// `iconst_*`/`bipush`/`sipush`: the fixture constants are small.
    fn push_int(&mut self, value: i32) {
        match value {
            -1 => self.op(0x02),
            0..=5 => self.op(0x03 + value as u8),
            -128..=127 => self.0.extend_from_slice(&[0x10, value as i8 as u8]),
            -32_768..=32_767 => self
                .0
                .extend_from_slice(&[0x11, (value >> 8) as u8, value as u8]),
            other => panic!("fixture needs a constant that fits a sipush operand: {other}"),
        }
    }

    fn aload(&mut self, slot: u8) {
        self.0.extend_from_slice(&[ALOAD, slot]);
    }

    fn astore(&mut self, slot: u8) {
        self.0.extend_from_slice(&[ASTORE, slot]);
    }

    fn newarray(&mut self, atype: u8) {
        self.0.extend_from_slice(&[NEWARRAY, atype]);
    }

    fn call(&mut self, opcode: u8, index: u16) {
        self.op_u16(opcode, index);
    }
}

fn jdk_home() -> std::ffi::OsString {
    std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    })
}

fn fixture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-arraycopy-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

fn int_field(builder: &mut ClassBuilder, name: &str) -> u16 {
    builder.field(0x0009, name, "I");
    builder.field_ref("ArrayCopy", name, "I")
}

fn string_field(builder: &mut ClassBuilder, name: &str) -> u16 {
    builder.field(0x0009, name, "Ljava/lang/String;");
    builder.field_ref("ArrayCopy", name, "Ljava/lang/String;")
}

fn iaload_into(code: &mut Code, slot: u8, index: i32, field: u16) {
    code.aload(slot);
    code.push_int(index);
    code.op(IALOAD);
    code.call(PUTSTATIC, field);
}

fn baload_into(code: &mut Code, slot: u8, index: i32, field: u16) {
    code.aload(slot);
    code.push_int(index);
    code.op(BALOAD);
    code.call(PUTSTATIC, field);
}

fn aaload_into(code: &mut Code, slot: u8, index: i32, field: u16) {
    code.aload(slot);
    code.push_int(index);
    code.op(AALOAD);
    code.call(PUTSTATIC, field);
}

/// Fills an `int[]` held in `slot` with `0..length`.
fn fill_ramp(code: &mut Code, slot: u8, length: i32) {
    for value in 0..length {
        code.aload(slot);
        code.push_int(value);
        code.push_int(value);
        code.op(IASTORE);
    }
}

/// One class whose `main` exercises every successful copy and leaves the results in statics.
///
/// Local slots: 1 `int[]` source, 2 `int[]` destination, 3 `byte[]` source, 4 `byte[]`
/// destination, 5 `String[]` source, 6 `String[]` destination, 7 overlapping array, 8 the
/// reverse-overlap array.
fn build_success(dir: &Path) {
    let mut builder = ClassBuilder::new("ArrayCopy", "java/lang/Object", 65);
    let ints: Vec<u16> = (0..5)
        .map(|k| int_field(&mut builder, &format!("I{k}")))
        .collect();
    let bytes: Vec<u16> = (0..4)
        .map(|k| int_field(&mut builder, &format!("B{k}")))
        .collect();
    let strings: Vec<u16> = (0..5)
        .map(|k| string_field(&mut builder, &format!("S{k}")))
        .collect();
    let overlap: Vec<u16> = (0..8)
        .map(|k| int_field(&mut builder, &format!("O{k}")))
        .collect();
    let reverse: Vec<u16> = (0..8)
        .map(|k| int_field(&mut builder, &format!("R{k}")))
        .collect();

    let arraycopy = builder.method_ref("java/lang/System", "arraycopy", ARRAYCOPY);
    let string_class = builder.class("java/lang/String");
    let literals: Vec<u16> = ["a", "b", "c"]
        .iter()
        .map(|text| builder.string(text))
        .collect();

    let mut code = Code::default();

    // int[]: src = [1, 2, 3, 4, 5], dest = new int[5], arraycopy(src, 1, dest, 0, 3).
    code.push_int(5);
    code.newarray(T_INT);
    code.astore(1);
    for (index, value) in (0..5).zip(1..=5) {
        code.aload(1);
        code.push_int(index);
        code.push_int(value);
        code.op(IASTORE);
    }
    code.push_int(5);
    code.newarray(T_INT);
    code.astore(2);
    code.aload(1);
    code.push_int(1);
    code.aload(2);
    code.push_int(0);
    code.push_int(3);
    code.call(INVOKESTATIC, arraycopy);
    for (index, field) in ints.iter().enumerate() {
        iaload_into(&mut code, 2, index as i32, *field);
    }

    // byte[]: src = [10, 20, 30, 40], dest = new byte[4], arraycopy(src, 1, dest, 1, 2).
    code.push_int(4);
    code.newarray(T_BYTE);
    code.astore(3);
    for (index, value) in [10, 20, 30, 40].iter().copied().enumerate() {
        code.aload(3);
        code.push_int(index as i32);
        code.push_int(value);
        code.op(BASTORE);
    }
    code.push_int(4);
    code.newarray(T_BYTE);
    code.astore(4);
    code.aload(3);
    code.push_int(1);
    code.aload(4);
    code.push_int(1);
    code.push_int(2);
    code.call(INVOKESTATIC, arraycopy);
    for (index, field) in bytes.iter().enumerate() {
        baload_into(&mut code, 4, index as i32, *field);
    }

    // String[]: src = ["a", "b", "c"], dest = new String[4], arraycopy(src, 1, dest, 2, 2).
    code.push_int(3);
    code.op_u16(ANEWARRAY, string_class);
    code.astore(5);
    for (index, literal) in literals.iter().enumerate() {
        code.aload(5);
        code.push_int(index as i32);
        code.op_u16(LDC_W, *literal);
        code.op(AASTORE);
    }
    code.push_int(4);
    code.op_u16(ANEWARRAY, string_class);
    code.astore(6);
    code.aload(5);
    code.push_int(1);
    code.aload(6);
    code.push_int(2);
    code.push_int(2);
    code.call(INVOKESTATIC, arraycopy);
    // S0 is the untouched slot 0 (null), S1/S2 the copied elements, S3/S4 the same object seen
    // through the source and the destination.
    aaload_into(&mut code, 6, 0, strings[0]);
    aaload_into(&mut code, 6, 2, strings[1]);
    aaload_into(&mut code, 6, 3, strings[2]);
    aaload_into(&mut code, 5, 1, strings[3]);
    aaload_into(&mut code, 6, 2, strings[4]);

    // Overlap, forward: O = [0..8], arraycopy(O, 0, O, 2, 5), then copy the moved window back
    // over [0..5] so the second copy is the reverse-overlap direction on the same array.
    code.push_int(8);
    code.newarray(T_INT);
    code.astore(7);
    fill_ramp(&mut code, 7, 8);
    for (from, to) in [(0, 2), (2, 0)] {
        code.aload(7);
        code.push_int(from);
        code.aload(7);
        code.push_int(to);
        code.push_int(5);
        code.call(INVOKESTATIC, arraycopy);
    }
    for (index, field) in overlap.iter().enumerate() {
        iaload_into(&mut code, 7, index as i32, *field);
    }

    // A pure backward move: R = [0..8], arraycopy(R, 2, R, 0, 5).
    code.push_int(8);
    code.newarray(T_INT);
    code.astore(8);
    fill_ramp(&mut code, 8, 8);
    code.aload(8);
    code.push_int(2);
    code.aload(8);
    code.push_int(0);
    code.push_int(5);
    code.call(INVOKESTATIC, arraycopy);
    for (index, field) in reverse.iter().enumerate() {
        iaload_into(&mut code, 8, index as i32, *field);
    }

    code.op(RETURN);
    builder.method(
        0x0009,
        "main",
        "([Ljava/lang/String;)V",
        Some((6, 9, code.0)),
    );
    std::fs::write(dir.join("ArrayCopy.class"), builder.build()).expect("fixture class");
}

/// Two arrays of the requested length in locals 1 and 2.
fn arrays(code: &mut Code, length: i32, source: u8, destination: u8) {
    code.push_int(length);
    code.newarray(source);
    code.astore(1);
    code.push_int(length);
    code.newarray(destination);
    code.astore(2);
}

fn build_error(dir: &Path, name: &str, body: fn(&mut Code, u16)) {
    let mut builder = ClassBuilder::new(name, "java/lang/Object", 65);
    let arraycopy = builder.method_ref("java/lang/System", "arraycopy", ARRAYCOPY);
    let mut code = Code::default();
    body(&mut code, arraycopy);
    code.op(RETURN);
    builder.method(
        0x0009,
        "main",
        "([Ljava/lang/String;)V",
        Some((6, 3, code.0)),
    );
    std::fs::write(dir.join(format!("{name}.class")), builder.build()).expect("fixture class");
}

fn negative_source_position(code: &mut Code, arraycopy: u16) {
    arrays(code, 2, T_INT, T_INT);
    code.aload(1);
    code.push_int(-1);
    code.aload(2);
    code.push_int(0);
    code.push_int(1);
    code.call(INVOKESTATIC, arraycopy);
}

fn length_past_the_end(code: &mut Code, arraycopy: u16) {
    arrays(code, 3, T_INT, T_INT);
    code.aload(1);
    code.push_int(2);
    code.aload(2);
    code.push_int(0);
    code.push_int(2);
    code.call(INVOKESTATIC, arraycopy);
}

fn negative_length(code: &mut Code, arraycopy: u16) {
    arrays(code, 3, T_INT, T_INT);
    code.aload(1);
    code.push_int(0);
    code.aload(2);
    code.push_int(0);
    code.push_int(-1);
    code.call(INVOKESTATIC, arraycopy);
}

fn mismatched_kinds(code: &mut Code, arraycopy: u16) {
    arrays(code, 2, T_INT, T_BYTE);
    code.aload(1);
    code.push_int(0);
    code.aload(2);
    code.push_int(0);
    code.push_int(1);
    code.call(INVOKESTATIC, arraycopy);
}

fn null_source(code: &mut Code, arraycopy: u16) {
    arrays(code, 2, T_INT, T_INT);
    code.op(ACONST_NULL);
    code.push_int(0);
    code.aload(2);
    code.push_int(0);
    code.push_int(1);
    code.call(INVOKESTATIC, arraycopy);
}

fn null_destination(code: &mut Code, arraycopy: u16) {
    arrays(code, 2, T_INT, T_INT);
    code.aload(1);
    code.push_int(0);
    code.op(ACONST_NULL);
    code.push_int(0);
    code.push_int(1);
    code.call(INVOKESTATIC, arraycopy);
}

fn run(dir: &Path, class: &str) -> (Shared, Result<(), VmError>) {
    let path: ClassPath = rava_cli::class_path(Path::new(&jdk_home()), dir).expect("class path");
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    let result = rava_interp::run_main(&mut shared, &mut thread, &path, class, &[]);
    (shared, result)
}

fn int(shared: &Shared, name: &str) -> i32 {
    let id = shared.class_ids["ArrayCopy"];
    match shared.classes[id.index()].statics.get(name) {
        Some(Value::Int(value)) => *value,
        other => panic!("{name} is {other:?}"),
    }
}

fn reference(shared: &Shared, name: &str) -> ObjRef {
    let id = shared.class_ids["ArrayCopy"];
    match shared.classes[id.index()].statics.get(name) {
        Some(Value::Ref(value)) => *value,
        other => panic!("{name} is {other:?}"),
    }
}

fn thrown_class(shared: &Shared, error: VmError) -> String {
    let VmError::Thrown { object } = error else {
        panic!("expected a thrown object, got {error}");
    };
    let class = shared.class_of(object).expect("the throwable has a class");
    shared.classes[class.index()].name.clone()
}

fn expect_bounds(error: VmError, index: i32, length: usize) {
    match error {
        VmError::IndexOutOfBounds {
            index: actual_index,
            length: actual_length,
        } => {
            assert_eq!(actual_index, index, "reported index");
            assert_eq!(actual_length, length, "reported array length");
        }
        other => panic!("expected an out-of-bounds fault, got {other}"),
    }
}

#[test]
fn an_int_range_copies() {
    let dir = fixture_dir("ints");
    build_success(&dir);
    let (shared, result) = run(&dir, "ArrayCopy");
    result.unwrap_or_else(|error| panic!("ArrayCopy failed: {error}"));

    assert_eq!(
        [
            int(&shared, "I0"),
            int(&shared, "I1"),
            int(&shared, "I2"),
            int(&shared, "I3"),
            int(&shared, "I4"),
        ],
        [2, 3, 4, 0, 0],
        "arraycopy(src, 1, dest, 0, 3) must copy [2, 3, 4] and leave the tail untouched"
    );
}

#[test]
fn a_byte_range_copies() {
    let dir = fixture_dir("bytes");
    build_success(&dir);
    let (shared, result) = run(&dir, "ArrayCopy");
    result.unwrap_or_else(|error| panic!("ArrayCopy failed: {error}"));

    assert_eq!(
        [
            int(&shared, "B0"),
            int(&shared, "B1"),
            int(&shared, "B2"),
            int(&shared, "B3"),
        ],
        [0, 20, 30, 0],
        "arraycopy(src, 1, dest, 1, 2) must copy the two bytes and leave the ends"
    );
}

#[test]
fn a_reference_range_copies_the_elements_and_their_identity() {
    let dir = fixture_dir("references");
    build_success(&dir);
    let (shared, result) = run(&dir, "ArrayCopy");
    result.unwrap_or_else(|error| panic!("ArrayCopy failed: {error}"));

    assert_eq!(
        reference(&shared, "S0"),
        ObjRef::NULL,
        "dest[0] is untouched"
    );
    assert_eq!(
        shared
            .string_text(reference(&shared, "S1"))
            .expect("S1 is a String"),
        "b"
    );
    assert_eq!(
        shared
            .string_text(reference(&shared, "S2"))
            .expect("S2 is a String"),
        "c"
    );
    assert_eq!(
        reference(&shared, "S3"),
        reference(&shared, "S4"),
        "a reference copy stores the same object, not a copy of it"
    );
}

#[test]
fn an_overlapping_self_copy_moves_as_if_buffered() {
    let dir = fixture_dir("overlap");
    build_success(&dir);
    let (shared, result) = run(&dir, "ArrayCopy");
    result.unwrap_or_else(|error| panic!("ArrayCopy failed: {error}"));

    let overlap: Vec<i32> = (0..8).map(|k| int(&shared, &format!("O{k}"))).collect();
    assert_eq!(
        overlap,
        [0, 1, 2, 3, 4, 3, 4, 7],
        "arraycopy(O, 0, O, 2, 5) then arraycopy(O, 2, O, 0, 5); a forward byte-by-byte copy \
         would have overwritten the source it had not read yet"
    );

    let reverse: Vec<i32> = (0..8).map(|k| int(&shared, &format!("R{k}"))).collect();
    assert_eq!(
        reverse,
        [2, 3, 4, 5, 6, 5, 6, 7],
        "arraycopy(R, 2, R, 0, 5); a backward byte-by-byte copy would have overwritten the source"
    );
}

#[test]
fn a_negative_position_is_rejected() {
    let dir = fixture_dir("negative-pos");
    build_error(&dir, "NegativePosition", negative_source_position);
    let (_, result) = run(&dir, "NegativePosition");
    match result {
        Err(error) => expect_bounds(error, -1, 2),
        Ok(()) => panic!("a negative source position was accepted"),
    }
}

#[test]
fn a_length_past_the_end_is_rejected() {
    let dir = fixture_dir("past-end");
    build_error(&dir, "PastTheEnd", length_past_the_end);
    let (_, result) = run(&dir, "PastTheEnd");
    match result {
        Err(error) => expect_bounds(error, 4, 3),
        Ok(()) => panic!("a range past the end was accepted"),
    }
}

#[test]
fn a_negative_length_is_rejected() {
    let dir = fixture_dir("negative-length");
    build_error(&dir, "NegativeLength", negative_length);
    let (_, result) = run(&dir, "NegativeLength");
    match result {
        Err(error) => expect_bounds(error, -1, 3),
        Ok(()) => panic!("a negative length was accepted"),
    }
}

#[test]
fn mismatched_array_kinds_are_rejected() {
    let dir = fixture_dir("kinds");
    build_error(&dir, "MismatchedKinds", mismatched_kinds);
    let (shared, result) = run(&dir, "MismatchedKinds");
    match result {
        Err(error) => assert_eq!(
            thrown_class(&shared, error),
            "java/lang/ArrayStoreException",
            "int[] into byte[] is an ArrayStoreException"
        ),
        Ok(()) => panic!("an int[] was copied into a byte[]"),
    }
}

#[test]
fn a_null_source_is_a_null_pointer_exception() {
    let dir = fixture_dir("null-source");
    build_error(&dir, "NullSource", null_source);
    let (shared, result) = run(&dir, "NullSource");
    match result {
        Err(error) => assert_eq!(
            thrown_class(&shared, error),
            "java/lang/NullPointerException"
        ),
        Ok(()) => panic!("a null source array was accepted"),
    }
}

#[test]
fn a_null_destination_is_a_null_pointer_exception() {
    let dir = fixture_dir("null-destination");
    build_error(&dir, "NullDestination", null_destination);
    let (shared, result) = run(&dir, "NullDestination");
    match result {
        Err(error) => assert_eq!(
            thrown_class(&shared, error),
            "java/lang/NullPointerException"
        ),
        Ok(()) => panic!("a null destination array was accepted"),
    }
}
