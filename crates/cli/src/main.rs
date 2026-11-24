//! `rava` — the command-line launcher.
//!
//! Deliberately dependency-light: M1 needs only the `compat` commands, so argument
//! handling is a small match instead of a CLI framework.

mod c1;
mod check;
mod compat;
mod ct_sym;
mod manifest;
mod run;
mod stubs;
mod surface;
mod target;

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        print_usage();
        return ExitCode::from(2);
    };
    let rest: Vec<String> = args.collect();

    match command.as_str() {
        "compat" => compat::run(&rest),
        "run" => run::command(&rest),
        "-h" | "--help" | "help" => {
            print_usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("error: unknown command `{other}`");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    println!("usage: rava <command>");
    println!();
    println!("commands:");
    println!("  run                run a class's main method from a classpath");
    println!("  compat target      print and validate the pinned compatibility target");
    println!("  compat manifest    generate or check the static surface manifest");
    println!("  compat stubs       write the signature-only stub class library");
    println!("  compat link-check  resolve the manifest against the stub class library");
    println!("  compat check       run the local M1 compatibility gate");
    println!("  compat c1-stubs    write the JDK 17 C1 signature stub library");
    println!("  compat metadata    scan or --check the C1 metadata against ct.sym");
}
