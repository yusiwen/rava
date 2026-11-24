//! `rava compat` — compatibility tooling.
//!
//! M1.1 adds `compat target`; the manifest (M1.2) and link-check (M1.3) commands
//! join it later.

use std::process::ExitCode;

use crate::target;

pub fn run(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("target") => target::command(&args[1..]),
        Some("-h" | "--help" | "help") => {
            print_usage();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("error: unknown compat command `{other}`");
            print_usage();
            ExitCode::from(2)
        }
        None => {
            eprintln!("error: missing compat command");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    println!("usage: rava compat <command>");
    println!();
    println!("commands:");
    println!("  target   print the pinned target and validate the target and jar locks");
}
