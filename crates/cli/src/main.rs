//! `rava` — command-line entry point.
//!
//! - `rava --dump <file.class>` prints the parsed structure, the debugging view for the
//!   parser.
//! - `rava run <Class>` executes `main` on the VM and writes its output to stdout.

use std::collections::BTreeMap;
use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use rava_classfile::{ClassFile, Constant, Error};
use rava_vm::ClassPath;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [] => {
            usage();
            ExitCode::from(2)
        }
        [flag, path] if flag == "--dump" => dump(path),
        [command, rest @ ..] if command == "run" => run(rest),
        [command, rest @ ..] if command == "bench" => bench(rest),
        [command, rest @ ..] if command == "verify" => verify(rest),
        [command, rest @ ..] if command == "module" => module_descriptor(rest),
        [command, rest @ ..] if command == "disasm" => disassemble(rest),
        [path] if !path.starts_with('-') => dump(path),
        _ => {
            usage();
            ExitCode::from(2)
        }
    }
}

fn usage() {
    eprintln!("usage:");
    eprintln!("  rava --dump <file.class>");
    eprintln!("  rava run <Class> [--home <jdk-home>] [--cp <dir>]");
    eprintln!("  rava bench <Class> [--iterations <n>] [--home <jdk-home>] [--cp <dir>]");
    eprintln!("  rava verify <Class> [--home <jdk-home>] [--cp <dir>]");
    eprintln!("  rava module <path/to/module-info.class>");
    eprintln!("  rava disasm <Class> [--home <jdk-home>] [--cp <dir>]");
}

fn run(args: &[String]) -> ExitCode {
    let mut home = PathBuf::from(".");
    let mut classpath = PathBuf::from(".");
    let mut class = None;
    let mut program_args: Vec<String> = Vec::new();

    let mut rest = args.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            // Everything after `--` belongs to the program, not to `rava`.
            "--" => {
                program_args.extend(rest.by_ref().cloned());
                break;
            }
            "--home" => match rest.next() {
                Some(value) => home = PathBuf::from(value),
                None => {
                    eprintln!("error: --home needs a JDK home or image directory");
                    return ExitCode::from(2);
                }
            },
            "--cp" => match rest.next() {
                Some(value) => classpath = PathBuf::from(value),
                None => {
                    eprintln!("error: --cp needs a directory");
                    return ExitCode::from(2);
                }
            },
            other => class = Some(other.to_owned()),
        }
    }

    let Some(class) = class else {
        eprintln!("error: run needs a class name");
        return ExitCode::from(2);
    };

    let mut shared = rava_vm::Shared::new();
    let mut thread = rava_vm::ThreadState::new();
    rava_native::register_jcl_natives(&mut shared);
    let path = ClassPath::new(&home, &classpath);

    match rava_interp::run_main(&mut shared, &mut thread, &path, &class, &program_args) {
        Ok(()) => {
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(&shared.out);
            let _ = stdout.flush();
            ExitCode::SUCCESS
        }
        // An uncaught throwable is a program failure, not a VM failure: report it the way Java
        // does, with the trace captured when it was thrown.
        Err(rava_vm::VmError::Thrown { object }) => {
            eprintln!("{}", shared.describe_throwable(object));
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Prints a class's decoded instructions with their stack effect and source line.
///
/// This is the form the interpreter matches on, not JVM mnemonics: the point is to show what
/// `rava-vm` decided at load time, and the numbers are the verifier's own table.
fn disassemble(args: &[String]) -> ExitCode {
    let mut class = None;
    let mut home = PathBuf::from(".");
    let mut classpath = PathBuf::from(".");
    let mut rest = args.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--home" => match rest.next() {
                Some(value) => home = PathBuf::from(value),
                None => {
                    eprintln!("error: --home needs a JDK home or image directory");
                    return ExitCode::from(2);
                }
            },
            "--cp" => match rest.next() {
                Some(value) => classpath = PathBuf::from(value),
                None => {
                    eprintln!("error: --cp needs a directory");
                    return ExitCode::from(2);
                }
            },
            other => class = Some(other.to_owned()),
        }
    }
    let Some(class) = class else {
        eprintln!("error: disasm needs a class name");
        return ExitCode::from(2);
    };

    let mut shared = rava_vm::Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let path = ClassPath::new(&home, &classpath);
    let name = class.replace('.', "/");
    let class_id = match shared.load_class(&path, &name) {
        Ok(class) => class,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let method_count = shared.class(class_id).methods.len();
    for index in 0..method_count {
        let (method_name, descriptor, code, line_at) = {
            let method = &shared.class(class_id).methods[index];
            let lines = method.lines.clone();
            let line_at =
                move |offset: u32| match lines.binary_search_by_key(&offset, |(entry, _)| *entry) {
                    Ok(position) => Some(lines[position].1),
                    Err(0) => lines.first().map(|(_, line)| *line),
                    Err(position) => Some(lines[position - 1].1),
                };
            (
                method.name.clone(),
                method.descriptor.clone(),
                method.code.clone(),
                line_at,
            )
        };
        let Some(code) = code else {
            println!("method {method_name}{descriptor}  (no body)");
            continue;
        };
        println!(
            "method {method_name}{descriptor}  max_stack={} max_locals={}",
            code.max_stack, code.max_locals
        );
        for (offset, instruction) in code.instrs.iter().enumerate() {
            let line = line_at(offset as u32);
            let effect = rava_vm::verify::stack_effect(instruction, &code, shared.class(class_id))
                .map(|(pop, push)| format!("pop {pop} push {push}"))
                .unwrap_or_else(|error| error.to_string());
            let line = line
                .map(|line| format!("line {line}"))
                .unwrap_or_else(|| "line ?".to_owned());
            println!(
                "  {:>4}  {:<44}  {:<14} {line}",
                code.pc(offset),
                format!("{instruction:?}"),
                effect
            );
        }
        // The exception table is what makes `try`/`catch`/`finally` visible: Java's compiler emits
        // it, not instructions, for the control flow an exception takes (JVMS §4.7.3).
        if !code.handlers.is_empty() {
            println!("  exception table:");
            for handler in &code.handlers {
                let kind = if handler.catch_type == 0 {
                    "any".to_owned()
                } else {
                    let constant = shared.class(class_id).file.as_ref().map(|file| {
                        file.constant_pool
                            .class_name(handler.catch_type)
                            .unwrap_or("<bad>")
                            .to_owned()
                    });
                    constant.unwrap_or_else(|| "<none>".to_owned())
                };
                println!(
                    "    [{:>4}..{:<4})  -> {:>4}   catch {kind}",
                    code.pc(handler.start),
                    code.pc(handler.end),
                    code.pc(handler.handler)
                );
            }
        }
        println!();
    }
    ExitCode::SUCCESS
}

/// Reads a module descriptor and prints what it declares (G6's first increment).
fn module_descriptor(args: &[String]) -> ExitCode {
    let Some(path) = args.first() else {
        eprintln!("error: module needs a path to a module-info.class");
        return ExitCode::from(2);
    };
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let file = match rava_classfile::ClassFile::parse(&bytes) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let Some(module) = (match file.module() {
        Ok(module) => module,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    }) else {
        println!("not a module descriptor");
        return ExitCode::SUCCESS;
    };

    println!("module {}", module.name);
    if let Some(version) = &module.version {
        println!("version {version}");
    }
    for requires in &module.requires {
        let version = requires
            .version
            .as_deref()
            .map(|version| format!(" ({version})"))
            .unwrap_or_default();
        println!("requires {}{version}", requires.name);
    }
    for export in &module.exports {
        println!("exports {}", export.package);
    }
    for open in &module.opens {
        println!("opens {}", open.package);
    }
    for service in &module.uses {
        println!("uses {service}");
    }
    for provides in &module.provides {
        println!(
            "provides {} with {}",
            provides.service,
            provides.with.join(", ")
        );
    }
    ExitCode::SUCCESS
}

/// Verifies every method body of a class without running it (G1).
fn verify(args: &[String]) -> ExitCode {
    let mut class = None;
    let mut home = PathBuf::from(".");
    let mut classpath = PathBuf::from(".");

    let mut rest = args.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--home" => match rest.next() {
                Some(value) => home = PathBuf::from(value),
                None => {
                    eprintln!("error: --home needs a JDK home or image directory");
                    return ExitCode::from(2);
                }
            },
            "--cp" => match rest.next() {
                Some(value) => classpath = PathBuf::from(value),
                None => {
                    eprintln!("error: --cp needs a directory");
                    return ExitCode::from(2);
                }
            },
            other => class = Some(other.to_owned()),
        }
    }
    let Some(class) = class else {
        eprintln!("error: verify needs a class name");
        return ExitCode::from(2);
    };

    let mut shared = rava_vm::Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    // The point of `verify` is to inspect a class that loading would refuse.
    shared.verify_on_load = false;
    let path = ClassPath::new(&home, &classpath);
    let name = class.replace('.', "/");
    let class_id = match shared.load_class(&path, &name) {
        Ok(class) => class,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let mut checked = 0usize;
    let method_count = shared.class(class_id).methods.len();
    for index in 0..method_count {
        let (method_name, descriptor, code) = {
            let method = &shared.class(class_id).methods[index];
            (
                method.name.clone(),
                method.descriptor.clone(),
                method.code.clone(),
            )
        };
        let Some(code) = code else { continue };
        let qualified = format!("{}.{}{}", class, method_name, descriptor);
        if let Err(error) = rava_vm::verify::verify(&code, shared.class(class_id), &qualified) {
            match error {
                rava_vm::VmError::BadConstantPool(detail) => eprintln!("rejected: {detail}"),
                other => eprintln!("rejected: {other}"),
            }
            return ExitCode::FAILURE;
        }
        checked += 1;
    }
    for check in rava_vm::verify::checks() {
        println!("checked  {check}");
    }
    println!("verified {checked} method(s) in {class}");
    ExitCode::SUCCESS
}

/// Runs a program `iterations` times and reports the cost per instruction.
///
/// This is measurement, not an optimisation target (G8): it records a baseline so a later
/// change can be compared against it, and so "the interpreter is slow" stops being a feeling.
fn bench(args: &[String]) -> ExitCode {
    let mut iterations: u32 = 5;
    let mut class = None;
    let mut home = PathBuf::from(".");
    let mut classpath = PathBuf::from(".");

    let mut rest = args.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--iterations" => match rest.next().and_then(|value| value.parse().ok()) {
                Some(value) => iterations = value,
                None => {
                    eprintln!("error: --iterations needs a number");
                    return ExitCode::from(2);
                }
            },
            "--home" => match rest.next() {
                Some(value) => home = PathBuf::from(value),
                None => {
                    eprintln!("error: --home needs a JDK home or image directory");
                    return ExitCode::from(2);
                }
            },
            "--cp" => match rest.next() {
                Some(value) => classpath = PathBuf::from(value),
                None => {
                    eprintln!("error: --cp needs a directory");
                    return ExitCode::from(2);
                }
            },
            other => class = Some(other.to_owned()),
        }
    }

    let Some(class) = class else {
        eprintln!("error: bench needs a class name");
        return ExitCode::from(2);
    };

    let path = ClassPath::new(&home, &classpath);
    let mut best_ns_per_step = f64::MAX;
    let mut steps = 0u64;
    let mut best_total = 0u128;
    let mut hot: Vec<(String, u64)> = Vec::new();

    for _ in 0..iterations {
        let mut shared = rava_vm::Shared::new();
        let mut thread = rava_vm::ThreadState::new();
        rava_native::register_jcl_natives(&mut shared);

        let started = std::time::Instant::now();
        if let Err(error) = rava_interp::run_main(&mut shared, &mut thread, &path, &class, &[]) {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
        let elapsed = started.elapsed();
        steps = shared.steps;
        // The trigger policy for a JIT is `count >= threshold`; recording the counts is what makes
        // that policy a measurement rather than a guess (G3's prerequisite).
        hot = shared.hot_methods(5);

        let nanoseconds = elapsed.as_nanos();
        let per_step = nanoseconds as f64 / shared.steps.max(1) as f64;
        if per_step < best_ns_per_step {
            best_ns_per_step = per_step;
            best_total = nanoseconds;
        }
    }

    println!("program    {class}");
    println!("iterations {iterations}");
    println!("steps      {steps}");
    println!(
        "best       {:.3} ms total, {:.2} ns per instruction",
        best_total as f64 / 1_000_000.0,
        best_ns_per_step
    );
    for (name, count) in &hot {
        println!("hot        {count:>9}  {name}");
    }
    ExitCode::SUCCESS
}

fn dump(path: &str) -> ExitCode {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("error: cannot read {path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    match ClassFile::parse(&bytes).and_then(|class| write_dump(&class)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {path}: {error}");
            ExitCode::FAILURE
        }
    }
}

fn write_dump(class: &ClassFile) -> Result<(), Error> {
    let this = class.this_class_name()?;
    let super_name = class.super_class_name()?.unwrap_or("<none>");
    let interfaces = class.interface_names()?;
    let usable: usize = class.constant_pool.iter().count();

    println!(
        "class {this}  major={0} minor={1}",
        class.major_version, class.minor_version
    );
    println!(
        "  access=0x{:04X} super={super_name} interfaces={}",
        class.access_flags,
        interfaces.len()
    );

    let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (_, entry) in class.constant_pool.iter() {
        *kinds.entry(kind_name(entry)).or_insert(0) += 1;
    }
    let summary: Vec<String> = kinds
        .iter()
        .map(|(kind, count)| format!("{kind}={count}"))
        .collect();
    println!(
        "  constants: {usable} usable in {} slot(s) [{}]",
        class.constant_pool.len(),
        summary.join(" ")
    );

    println!("  fields: {}", class.fields.len());
    for field in &class.fields {
        println!(
            "    {} {} flags=0x{:04X}",
            class.field_name(field)?,
            class.field_descriptor(field)?,
            field.access_flags
        );
    }

    println!("  methods: {}", class.methods.len());
    for method in &class.methods {
        let attributes = attribute_names(class, &method.attributes)?;
        println!(
            "    {} {} flags=0x{:04X} attributes=[{}]",
            class.method_name(method)?,
            class.method_descriptor(method)?,
            method.access_flags,
            attributes.join(", ")
        );
    }

    let attributes = attribute_names(class, &class.attributes)?;
    println!("  attributes: [{}]", attributes.join(", "));
    Ok(())
}

fn attribute_names<'a>(
    class: &'a ClassFile,
    attributes: &[rava_classfile::Attribute],
) -> Result<Vec<&'a str>, Error> {
    attributes
        .iter()
        .map(|attribute| class.attribute_name(attribute))
        .collect()
}

fn kind_name(entry: &Constant) -> &'static str {
    match entry {
        Constant::Utf8(_) => "utf8",
        Constant::Integer(_) => "int",
        Constant::Float(_) => "float",
        Constant::Long(_) => "long",
        Constant::Double(_) => "double",
        Constant::Class { .. } => "class",
        Constant::String { .. } => "string",
        Constant::FieldRef { .. } => "fieldref",
        Constant::MethodRef { .. } => "methodref",
        Constant::InterfaceMethodRef { .. } => "interfacemethodref",
        Constant::NameAndType { .. } => "nameandtype",
        Constant::MethodHandle { .. } => "methodhandle",
        Constant::MethodType { .. } => "methodtype",
        Constant::Dynamic { .. } => "dynamic",
        Constant::InvokeDynamic { .. } => "invokedynamic",
        Constant::Module { .. } => "module",
        Constant::Package { .. } => "package",
    }
}
