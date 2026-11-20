//! S19 acceptance: `Throwable.fillInStackTrace` fills the host's `stackTrace` field, so Java
//! code — `getStackTrace()` included — reads real `StackTraceElement` objects.
//!
//! The fixtures throw an `IllegalStateException`, catch it, and store into static fields the
//! answers Java code computes from `getStackTrace()`: the length, the innermost frame's class
//! and method, the class of the frame whose method is `fillInStackTrace`, and the outermost
//! frame (the throwing method). The message and `toString` are stored too, to prove the throwable
//! still behaves after the array is filled.
//!
//! `getStackTrace()` clones the array before returning it, so this also pins the array half of
//! `Object.clone`. The class files are built by the class-file builder: the tests need no JDK
//! sources, only `JAVA21_HOME` at run time for the host library.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rava_classfile::ClassBuilder;
use rava_vm::{ClassPath, Shared, ThreadState, Value, VmError};

fn host_path(dir: &Path) -> ClassPath {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    rava_cli::class_path(Path::new(&home), dir).expect("class path")
}

fn fixture_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("rava-stack-traces-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

fn write(dir: &Path, name: &str, bytes: Vec<u8>) {
    std::fs::write(dir.join(format!("{name}.class")), bytes).expect("fixture class");
}

fn run(path: &ClassPath, class: &str) -> (Shared, ThreadState) {
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    if let Err(error) = rava_interp::run_main(&mut shared, &mut thread, path, class, &[]) {
        if let VmError::Thrown { object } = &error {
            let thrown = shared
                .class_of(*object)
                .map(|class| shared.class(class).name.clone());
            panic!("{class} failed with an uncaught {thrown:?}: {error}");
        }
        panic!("{class} failed: {error}");
    }
    (shared, thread)
}

// --- a very small instruction assembler -------------------------------------------------------
//
// `ClassBuilder` owns the constant-pool indices but not labels: a method body is raw bytes and a
// branch offset is relative to its own opcode. `mark`/`goto`/`ifeq` record the position and patch
// it in `done`, so the fixtures cannot get an offset wrong by hand.

struct Code {
    bytes: Vec<u8>,
    labels: HashMap<&'static str, usize>,
    fixups: Vec<(&'static str, usize)>,
}

impl Code {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            labels: HashMap::new(),
            fixups: Vec::new(),
        }
    }

    fn op(&mut self, opcode: u8) {
        self.bytes.push(opcode);
    }

    fn u16(&mut self, opcode: u8, operand: u16) {
        self.bytes
            .extend_from_slice(&[opcode, (operand >> 8) as u8, operand as u8]);
    }

    fn iconst(&mut self, value: i32) {
        match value {
            -1 => self.op(0x02),
            0..=5 => self.op(0x03 + value as u8),
            _ => self.u16(0x11, value as i16 as u16), // sipush
        }
    }

    /// `iload`/`istore`/`aload`/`astore` in the short forms, generic form above 3.
    fn local(&mut self, opcode: u8, short: u8, slot: u8) {
        if slot <= 3 {
            self.op(short + slot);
        } else {
            self.bytes.extend_from_slice(&[opcode, slot]);
        }
    }

    fn iload(&mut self, slot: u8) {
        self.local(0x15, 0x1a, slot);
    }

    fn istore(&mut self, slot: u8) {
        self.local(0x36, 0x3b, slot);
    }

    fn aload(&mut self, slot: u8) {
        self.local(0x19, 0x2a, slot);
    }

    fn astore(&mut self, slot: u8) {
        self.local(0x3a, 0x4b, slot);
    }

    fn mark(&mut self, label: &'static str) {
        self.labels.insert(label, self.bytes.len());
    }

    fn branch(&mut self, opcode: u8, label: &'static str) {
        let start = self.bytes.len();
        self.bytes.extend_from_slice(&[opcode, 0x00, 0x00]);
        self.fixups.push((label, start));
    }

    fn goto(&mut self, label: &'static str) {
        self.branch(0xa7, label);
    }

    fn ifeq(&mut self, label: &'static str) {
        self.branch(0x99, label);
    }

    fn if_icmpge(&mut self, label: &'static str) {
        self.branch(0xa2, label);
    }

    fn done(mut self) -> Vec<u8> {
        for (label, start) in self.fixups {
            let target = *self
                .labels
                .get(label)
                .unwrap_or_else(|| panic!("branch to undefined label {label}"));
            let offset = (target as isize - start as isize) as i16;
            self.bytes[start + 1..start + 3].copy_from_slice(&offset.to_be_bytes());
        }
        self.bytes
    }
}

/// The references a fixture body needs; resolved once so the bytecode is readable.
struct Refs {
    exception: u16,
    init: u16,
    get_stack_trace: u16,
    get_class_name: u16,
    get_method_name: u16,
    get_message: u16,
    to_string: u16,
    string_equals: u16,
    boom: u16,
    fill_in_stack_trace: u16,
}

fn refs(builder: &mut ClassBuilder) -> Refs {
    Refs {
        exception: builder.class("java/lang/IllegalStateException"),
        init: builder.method_ref(
            "java/lang/IllegalStateException",
            "<init>",
            "(Ljava/lang/String;)V",
        ),
        get_stack_trace: builder.method_ref(
            "java/lang/Throwable",
            "getStackTrace",
            "()[Ljava/lang/StackTraceElement;",
        ),
        get_class_name: builder.method_ref(
            "java/lang/StackTraceElement",
            "getClassName",
            "()Ljava/lang/String;",
        ),
        get_method_name: builder.method_ref(
            "java/lang/StackTraceElement",
            "getMethodName",
            "()Ljava/lang/String;",
        ),
        get_message: builder.method_ref(
            "java/lang/Throwable",
            "getMessage",
            "()Ljava/lang/String;",
        ),
        to_string: builder.method_ref("java/lang/Throwable", "toString", "()Ljava/lang/String;"),
        string_equals: builder.method_ref("java/lang/String", "equals", "(Ljava/lang/Object;)Z"),
        boom: builder.string("boom"),
        fill_in_stack_trace: builder.string("fillInStackTrace"),
    }
}

/// The shared prelude: `try { throw new IllegalStateException("boom"); } catch (...) { e = ... }`.
/// Leaves the caught throwable in local 1 and returns the exception table entry.
fn throw_and_catch(code: &mut Code, refs: &Refs) -> (u16, u16, u16, u16) {
    let start = code.bytes.len();
    code.u16(0xbb, refs.exception); // new
    code.op(0x59); // dup
    code.u16(0x13, refs.boom); // ldc_w "boom"
    code.u16(0xb7, refs.init); // invokespecial
    code.op(0xbf); // athrow
    let end = code.bytes.len();
    let handler = code.bytes.len();
    code.astore(1); // the handler
    (start as u16, end as u16, handler as u16, refs.exception)
}

/// `StackTrace.main`: throw, catch, then record what `getStackTrace()` says.
fn build_stack_trace(dir: &Path) {
    let mut builder = ClassBuilder::new("StackTrace", "java/lang/Object", 65);
    builder.field(0x0009, "LEN", "I");
    for name in [
        "CLASS0",
        "METHOD0",
        "FILL",
        "LAST",
        "LASTMETHOD",
        "MSG",
        "TOSTR",
    ] {
        builder.field(0x0009, name, "Ljava/lang/String;");
    }
    let refs = refs(&mut builder);
    let len = builder.field_ref("StackTrace", "LEN", "I");
    let field = |builder: &mut ClassBuilder, name: &str| {
        builder.field_ref("StackTrace", name, "Ljava/lang/String;")
    };
    let class0 = field(&mut builder, "CLASS0");
    let method0 = field(&mut builder, "METHOD0");
    let fill = field(&mut builder, "FILL");
    let last = field(&mut builder, "LAST");
    let last_method = field(&mut builder, "LASTMETHOD");
    let msg = field(&mut builder, "MSG");
    let tostr = field(&mut builder, "TOSTR");

    let mut code = Code::new();
    let (start, end, handler, exception) = throw_and_catch(&mut code, &refs);

    code.aload(1);
    code.u16(0xb6, refs.get_stack_trace); // getStackTrace()
    code.astore(2);
    code.aload(2);
    code.op(0xbe); // arraylength
    code.u16(0xb3, len);

    // The innermost frame: the array's [0].
    code.aload(2);
    code.iconst(0);
    code.op(0x32); // aaload
    code.astore(3);
    code.aload(3);
    code.u16(0xb6, refs.get_class_name);
    code.u16(0xb3, class0);
    code.aload(3);
    code.u16(0xb6, refs.get_method_name);
    code.u16(0xb3, method0);

    // The outermost frame: the method that created (and threw) the throwable.
    code.aload(2);
    code.aload(2);
    code.op(0xbe);
    code.iconst(1);
    code.op(0x64); // isub
    code.op(0x32); // aaload
    code.astore(4);
    code.aload(4);
    code.u16(0xb6, refs.get_class_name);
    code.u16(0xb3, last);
    code.aload(4);
    code.u16(0xb6, refs.get_method_name);
    code.u16(0xb3, last_method);

    code.aload(1);
    code.u16(0xb6, refs.get_message);
    code.u16(0xb3, msg);
    code.aload(1);
    code.u16(0xb6, refs.to_string);
    code.u16(0xb3, tostr);

    // Find the frame whose method is `fillInStackTrace` (index 0 in practice, searched so the
    // test states the property rather than a position).
    code.iconst(0);
    code.istore(5);
    code.mark("loop");
    code.iload(5);
    code.aload(2);
    code.op(0xbe);
    code.if_icmpge("done");
    code.aload(2);
    code.iload(5);
    code.op(0x32); // aaload
    code.u16(0xb6, refs.get_method_name);
    code.u16(0x13, refs.fill_in_stack_trace);
    code.u16(0xb6, refs.string_equals);
    code.ifeq("next");
    code.aload(2);
    code.iload(5);
    code.op(0x32);
    code.u16(0xb6, refs.get_class_name);
    code.u16(0xb3, fill);
    code.goto("done");
    code.mark("next");
    code.bytes.extend_from_slice(&[0x84, 5, 1]); // iinc 5, 1
    code.goto("loop");
    code.mark("done");
    code.op(0xb1); // return

    builder.method_with_handlers(
        0x0009,
        "main",
        "([Ljava/lang/String;)V",
        Some((4, 6, code.done())),
        vec![(start, end, handler, exception)],
    );
    write(dir, "StackTrace", builder.build());
}

/// `StackTraceTwice.main`: two `getStackTrace()` calls must see the same VM trace.
fn build_stack_trace_twice(dir: &Path) {
    let mut builder = ClassBuilder::new("StackTraceTwice", "java/lang/Object", 65);
    builder.field(0x0009, "LEN1", "I");
    builder.field(0x0009, "LEN2", "I");
    for name in ["CLASS1", "CLASS2", "METHOD1", "METHOD2"] {
        builder.field(0x0009, name, "Ljava/lang/String;");
    }
    let refs = refs(&mut builder);
    let len1 = builder.field_ref("StackTraceTwice", "LEN1", "I");
    let len2 = builder.field_ref("StackTraceTwice", "LEN2", "I");
    let field = |builder: &mut ClassBuilder, name: &str| {
        builder.field_ref("StackTraceTwice", name, "Ljava/lang/String;")
    };
    let class1 = field(&mut builder, "CLASS1");
    let class2 = field(&mut builder, "CLASS2");
    let method1 = field(&mut builder, "METHOD1");
    let method2 = field(&mut builder, "METHOD2");

    let mut code = Code::new();
    let (start, end, handler, exception) = throw_and_catch(&mut code, &refs);

    code.aload(1);
    code.u16(0xb6, refs.get_stack_trace);
    code.astore(2);
    code.aload(1);
    code.u16(0xb6, refs.get_stack_trace);
    code.astore(3);
    code.aload(2);
    code.op(0xbe);
    code.u16(0xb3, len1);
    code.aload(3);
    code.op(0xbe);
    code.u16(0xb3, len2);

    for (array, class_field, method_field) in [(2u8, class1, method1), (3u8, class2, method2)] {
        code.aload(array);
        code.iconst(0);
        code.op(0x32); // aaload
        code.u16(0xb6, refs.get_class_name);
        code.u16(0xb3, class_field);
        code.aload(array);
        code.iconst(0);
        code.op(0x32);
        code.u16(0xb6, refs.get_method_name);
        code.u16(0xb3, method_field);
    }
    code.op(0xb1); // return

    builder.method_with_handlers(
        0x0009,
        "main",
        "([Ljava/lang/String;)V",
        Some((3, 4, code.done())),
        vec![(start, end, handler, exception)],
    );
    write(dir, "StackTraceTwice", builder.build());
}

fn int(shared: &Shared, class: &str, name: &str) -> i32 {
    let id = shared.class_ids[class];
    match shared.classes[id.index()].statics.get(name) {
        Some(Value::Int(value)) => *value,
        other => panic!("{class}.{name} is {other:?}"),
    }
}

fn text(shared: &Shared, class: &str, name: &str) -> String {
    let id = shared.class_ids[class];
    match shared.classes[id.index()].statics.get(name) {
        Some(Value::Ref(reference)) => shared.string_text(*reference).expect("a String"),
        other => panic!("{class}.{name} is {other:?}"),
    }
}

#[test]
fn get_stack_trace_reads_the_frames_the_native_captured() {
    let dir = fixture_dir("single");
    build_stack_trace(&dir);
    let path = host_path(&dir);
    let (shared, _thread) = run(&path, "StackTrace");

    assert_eq!(int(&shared, "StackTrace", "LEN"), 6, "frame count");
    assert_eq!(
        text(&shared, "StackTrace", "CLASS0"),
        "java.lang.Throwable",
        "innermost frame class"
    );
    assert_eq!(
        text(&shared, "StackTrace", "METHOD0"),
        "fillInStackTrace",
        "innermost frame method"
    );
    assert_eq!(
        text(&shared, "StackTrace", "FILL"),
        "java.lang.Throwable",
        "the class of the frame whose method is fillInStackTrace"
    );
    assert_eq!(
        text(&shared, "StackTrace", "LAST"),
        "StackTrace",
        "outermost frame class: the throwing method"
    );
    assert_eq!(
        text(&shared, "StackTrace", "LASTMETHOD"),
        "main",
        "outermost frame method: the throwing method"
    );
    assert_eq!(text(&shared, "StackTrace", "MSG"), "boom", "getMessage");
    assert_eq!(
        text(&shared, "StackTrace", "TOSTR"),
        "java.lang.IllegalStateException: boom",
        "toString"
    );
}

#[test]
fn two_get_stack_trace_calls_see_the_same_trace() {
    let dir = fixture_dir("twice");
    build_stack_trace_twice(&dir);
    let path = host_path(&dir);
    let (shared, _thread) = run(&path, "StackTraceTwice");

    let len1 = int(&shared, "StackTraceTwice", "LEN1");
    assert_eq!(len1, 6, "first frame count");
    assert_eq!(
        int(&shared, "StackTraceTwice", "LEN2"),
        len1,
        "the second call sees the same frame count"
    );
    assert_eq!(
        (
            text(&shared, "StackTraceTwice", "CLASS1"),
            text(&shared, "StackTraceTwice", "METHOD1")
        ),
        (
            text(&shared, "StackTraceTwice", "CLASS2"),
            text(&shared, "StackTraceTwice", "METHOD2")
        ),
        "both calls read the same innermost frame"
    );
}
