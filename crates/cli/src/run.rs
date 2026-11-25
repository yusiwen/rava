//! `rava run` — run a class's `main([Ljava/lang/String;)V` from a classpath.
//!
//! The command is deliberately small: it opens the class library directory, adds one
//! application loader parented to the bootstrap loader (the classpath), registers the
//! library natives, resolves the main class through that loader and invokes `main` with
//! an ordinary `String[]` allocated by the heap. There is no second class table and no
//! second identity model; the application loader is a source in the existing
//! `(loader, name)` table.
//!
//! Exit codes are the contract:
//!
//! - `0`: `main` returned normally.
//! - `1`: runtime error (missing class, missing/non-static `main`, a load or execution
//!   failure).
//! - `2`: usage error (missing `MainClass`, an unknown option, a missing option value).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use rava_interp::run_static_method;
use rava_vm::{ClassPathSource, ClassRef, LoaderId, Shared, Value, VmError, library};

/// The Java entry-point descriptor.
const MAIN_DESCRIPTOR: &str = "([Ljava/lang/String;)V";

/// The default bootstrap class-library directory, relative to the process directory.
const DEFAULT_STDLIB: &str = "stdlib/c1/classes";

/// Parsed `rava run` options.
struct Options {
    main_class: String,
    stdlib: PathBuf,
    classpath: Vec<PathBuf>,
    program_args: Vec<String>,
}

enum Parsed {
    Run(Options),
    Help,
}

pub fn command(args: &[String]) -> ExitCode {
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("error: {message}");
            print_usage();
            return ExitCode::from(2);
        }
    };
    let options = match parsed {
        Parsed::Run(options) => options,
        Parsed::Help => {
            print_usage();
            return ExitCode::SUCCESS;
        }
    };
    match execute(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(1)
        }
    }
}

fn execute(options: &Options) -> Result<(), String> {
    let shared = Shared::with_library_dir(&options.stdlib);
    let classpath = ClassPathSource::from_paths(options.classpath.iter().cloned())
        .map_err(|error| format!("cannot build the classpath: {error}"))?;
    let loader = shared
        .add_loader(LoaderId::BOOTSTRAP, Arc::new(classpath))
        .map_err(|error| format!("cannot create the application loader: {error}"))?;
    library::register_all(&shared)
        .map_err(|error| format!("cannot register the class library: {error}"))?;
    // `Method.invoke`/`Constructor.newInstance` and
    // `InvocationHandler.invokeDefault` live in the interpreter because they re-enter it;
    // a runner that wants reflection must install them explicitly. The same is true of
    // `MethodHandle.invoke*` (M3.4).
    rava_interp::reflect_invoke::register_natives(&shared);
    rava_interp::method_handles::register_natives(&shared);
    rava_interp::unsafe_natives::register_natives(&shared)
        .map_err(|error| format!("cannot register the Unsafe natives: {error}"))?;
    rava_interp::proxy::register_natives(&shared);
    rava_interp::callbacks::register_natives(&shared);
    rava_interp::streams::register_natives(&shared);
    rava_interp::futures::register_natives(&shared);
    rava_interp::object_methods::register_natives(&shared);

    let main_class = match shared.load_class(loader, &options.main_class) {
        Ok(class) => class,
        Err(VmError::ClassNotFound { .. }) => {
            return Err(format!(
                "class {} not found on the classpath",
                options.main_class
            ));
        }
        Err(error) => {
            return Err(format!("cannot load class {}: {error}", options.main_class));
        }
    };

    let entry = shared
        .entry(main_class)
        .ok_or_else(|| format!("class {} is not loaded", options.main_class))?;
    let method = entry
        .file
        .find_method("main", MAIN_DESCRIPTOR)
        .map_err(|error| format!("cannot read {}: {error}", options.main_class))?;
    let Some(method) = method else {
        return Err(format!(
            "class {} has no main{MAIN_DESCRIPTOR} method",
            options.main_class
        ));
    };
    if !method.is_static() {
        return Err(format!(
            "class {} main{MAIN_DESCRIPTOR} is not static",
            options.main_class
        ));
    }

    // `main` receives an ordinary `String[]`, allocated from the heap with the same
    // component-class machinery `anewarray` uses. Program arguments are interned, so the
    // array holds the VM's canonical string objects.
    let string_class = shared
        .load_class(loader, "java/lang/String")
        .map_err(|error| format!("cannot load java.lang.String: {error}"))?;
    let arguments: Vec<rava_vm::ObjRef> = options
        .program_args
        .iter()
        .map(|argument| shared.intern_string(argument))
        .collect();
    let args_array = shared
        .heap()
        .allocate_array(ClassRef::Named(string_class), arguments.len());
    {
        let mut heap = shared.heap();
        let array = heap
            .array_mut(args_array)
            .map_err(|error| format!("cannot initialise the argument array: {error}"))?;
        for (slot, argument) in array.elements.iter_mut().zip(arguments) {
            *slot = Value::Ref(argument);
        }
    }

    let thread = shared.thread_state(1);
    let result = run_static_method(
        &shared,
        &thread,
        main_class,
        "main",
        MAIN_DESCRIPTOR,
        vec![Value::Ref(args_array)],
    );

    // Flush what the program printed even when it failed: a Java program that prints and
    // then throws has still printed. Diagnostics captured by `java.util.logging` go to
    // stderr, so they never disturb the program's `System.out`.
    use std::io::Write;
    let output = shared.stdout();
    if !output.is_empty() {
        std::io::stdout()
            .write_all(output.as_bytes())
            .map_err(|error| format!("cannot write stdout: {error}"))?;
    }
    let diagnostics = shared.stderr();
    if !diagnostics.is_empty() {
        std::io::stderr()
            .write_all(diagnostics.as_bytes())
            .map_err(|error| format!("cannot write stderr: {error}"))?;
    }

    result.map_err(|error| describe_error(&shared, error))?;
    Ok(())
}

/// Render an execution error, naming an uncaught Java exception by class and message.
fn describe_error(shared: &Shared, error: rava_interp::InterpError) -> String {
    if let rava_interp::InterpError::Thrown(throwable) = error {
        let class = shared
            .get_class(throwable)
            .ok()
            .and_then(|object| shared.heap().class(object).ok())
            .and_then(|class| {
                shared
                    .entry(class)
                    .map(|entry| entry.name.replace('/', "."))
            })
            .unwrap_or_else(|| "<unknown>".to_owned());
        return match shared.throwable_message(throwable) {
            Some(message) => format!("uncaught {class}: {message}"),
            None => format!("uncaught {class}"),
        };
    }
    error.to_string()
}

fn parse(args: &[String]) -> Result<Parsed, String> {
    let mut main_class = None;
    let mut stdlib = PathBuf::from(DEFAULT_STDLIB);
    let mut classpath = Vec::new();
    let mut program_args = Vec::new();
    let mut literal = false;
    let mut index = 0;

    while index < args.len() {
        let argument = args[index].as_str();
        if literal {
            match main_class {
                Some(_) => program_args.push(argument.to_owned()),
                None => main_class = Some(argument.to_owned()),
            }
            index += 1;
            continue;
        }
        match argument {
            "--" => {
                literal = true;
                index += 1;
            }
            "-h" | "--help" | "help" => return Ok(Parsed::Help),
            "--stdlib" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "`--stdlib` needs a directory".to_owned())?;
                stdlib = PathBuf::from(value);
                index += 2;
            }
            "--cp" | "-cp" | "--classpath" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "`--cp` needs a path list".to_owned())?;
                push_classpath(&mut classpath, value);
                index += 2;
            }
            other if other.starts_with("--stdlib=") => {
                stdlib = PathBuf::from(&other["--stdlib=".len()..]);
                index += 1;
            }
            other if other.starts_with("--cp=") => {
                push_classpath(&mut classpath, &other["--cp=".len()..]);
                index += 1;
            }
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("unknown option `{other}`"));
            }
            other => {
                match main_class {
                    Some(_) => program_args.push(other.to_owned()),
                    None => main_class = Some(other.to_owned()),
                }
                index += 1;
            }
        }
    }

    let main_class = main_class.ok_or_else(|| "missing <MainClass>".to_owned())?;
    Ok(Parsed::Run(Options {
        main_class,
        stdlib,
        classpath,
        program_args,
    }))
}

/// Append a colon-separated classpath list. Empty entries are ignored.
fn push_classpath(classpath: &mut Vec<PathBuf>, value: &str) {
    for entry in value.split(':') {
        if !entry.is_empty() {
            classpath.push(PathBuf::from(entry));
        }
    }
}

fn print_usage() {
    println!("usage: rava run <MainClass> [options] [args...]");
    println!();
    println!("runs the static main([Ljava/lang/String;)V method of <MainClass>.");
    println!();
    println!("options:");
    println!("  --stdlib <dir>    bootstrap class-library directory (default: {DEFAULT_STDLIB})");
    println!("  --cp <path-list>  classpath entries, colon-separated or repeated (default: empty)");
    println!("  --                treat every following argument as a program argument");
}
