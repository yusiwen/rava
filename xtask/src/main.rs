//! Repository tasks, run as `cargo xtask <task>` (see `.cargo/config.toml`).
//!
//! `ci` is the single acceptance entry point recorded in `PROJECT.md`: it runs the
//! formatting check, the linter, the tests and the foundation invariants, in that order,
//! and stops at the first failure so the first error is the visible one.

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_else(|| "ci".to_owned());
    let code = match task.as_str() {
        "ci" => ci(),
        "fmt" => run("cargo", &["fmt", "--all"]),
        "clippy" => run("cargo", &["clippy", "--workspace", "--all-targets"]),
        "test" => run("cargo", &["test", "--workspace"]),
        "invariants" => run("bash", &["scripts/check-invariants.sh"]),
        other => {
            eprintln!("unknown task: {other}");
            eprintln!("tasks: ci, fmt, clippy, test, invariants");
            return ExitCode::from(2);
        }
    };
    ExitCode::from(code)
}

fn ci() -> u8 {
    let mut steps: Vec<(&str, Vec<&str>)> = vec![("cargo", vec!["fmt", "--all", "--check"])];

    // Clippy is a rustup component and is absent on some toolchains. Its absence is
    // announced rather than silently dropped, so a lint regression cannot hide behind a
    // missing tool.
    if tool_works("cargo", &["clippy", "--version"]) {
        steps.push(("cargo", vec!["clippy", "--workspace", "--all-targets"]));
    } else {
        println!("warn: clippy is not installed; skipping lints (rustup component add clippy)");
    }

    steps.push(("cargo", vec!["test", "--workspace"]));
    steps.push(("bash", vec!["scripts/check-invariants.sh"]));

    for (program, args) in steps {
        let code = run(program, &args);
        if code != 0 {
            return code;
        }
    }
    0
}

fn tool_works(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn run(program: &str, args: &[&str]) -> u8 {
    println!("$ {program} {}", args.join(" "));
    match Command::new(program).args(args).status() {
        Ok(status) if status.success() => 0,
        Ok(status) => status.code().unwrap_or(1).clamp(0, 255) as u8,
        Err(error) => {
            eprintln!("error: failed to run {program}: {error}");
            1
        }
    }
}
