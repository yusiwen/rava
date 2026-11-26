//! Repository tasks, run as `cargo xtask <task>`.
//!
//! `ci` is the local acceptance entry point: document budgets, formatting, lints and
//! tests, stopping at the first failure.

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_else(|| "ci".to_owned());
    let code = match task.as_str() {
        "ci" => ci(),
        "fmt" => run("cargo", &["fmt", "--all"]),
        "clippy" => run(
            "cargo",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        "test" => run("cargo", &["test", "--workspace"]),
        other => {
            eprintln!("unknown task: {other}");
            eprintln!("tasks: ci, fmt, clippy, test");
            return ExitCode::from(2);
        }
    };
    ExitCode::from(code)
}

fn ci() -> u8 {
    let steps: Vec<(&str, Vec<&str>)> = vec![
        // Cheapest check first: it only reads Markdown.
        ("bash", vec!["scripts/doc-check.sh", "."]),
        ("cargo", vec!["fmt", "--all", "--check"]),
        (
            "cargo",
            vec![
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("cargo", vec!["test", "--workspace"]),
    ];
    for (program, args) in steps {
        let code = run(program, &args);
        if code != 0 {
            return code;
        }
    }
    0
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
