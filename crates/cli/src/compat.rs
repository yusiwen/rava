//! `rava compat` — compatibility tooling.
//!
//! M1.1 adds `compat target`, M1.2 adds `compat manifest`, M1.3 adds `compat stubs`
//! and `compat link-check`, and M1.4 adds `compat check`, which runs all of them.

use std::process::ExitCode;

use crate::{check, manifest, stubs, target};

pub fn run(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("target") => target::command(&args[1..]),
        Some("manifest") => manifest::command(&args[1..]),
        Some("stubs") => stubs::stubs_command(&args[1..]),
        Some("link-check") => stubs::link_check_command(&args[1..]),
        Some("check") => check::command(&args[1..]),
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
    println!("  target      print the pinned target and validate the target and jar locks");
    println!("  manifest    generate the static surface manifest, or --check the committed one");
    println!("  stubs       write the signature-only stub class library");
    println!("  link-check  resolve the manifest against the stub class library");
    println!("  check       run the local M1 compatibility gate");
}
