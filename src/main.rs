//! Phorge CLI: `phorge <run|runvm|check|parse|lex|transpile|bench> <file>`. Thin dispatcher
//! over the testable `phorge::cli` module.
#![forbid(unsafe_code)]

use std::process::exit;

use phorge::cli;

const USAGE: &str = "usage: phorge <run|runvm|check|parse|lex|transpile|bench|build> \
                     <file | - | -e code> [-o out]   (phorge -h for help, -v for version)";

fn main() {
    // Self-executing artifact: if this binary carries an embedded program, run it on the VM and
    // exit before any CLI parsing. No payload -> fall through to the normal dispatcher.
    if let Some(src) = phorge::bundle::embedded_source() {
        match cli::cmd_runvm(&src) {
            Ok(text) => {
                print!("{text}");
                exit(0);
            }
            Err(err) => {
                eprintln!("{err}");
                exit(1);
            }
        }
    }

    let args: Vec<String> = std::env::args().collect();
    // Global flags (before subcommand dispatch): -h/--help and -v/--version print and exit 0.
    match args.get(1).map(String::as_str) {
        Some("-h" | "--help") => {
            print!("{}", cli::help_text());
            return;
        }
        Some("-v" | "--version") => {
            println!("{}", cli::version_line());
            return;
        }
        _ => {}
    }
    let cmd = match args.get(1).map(String::as_str) {
        Some(
            c @ ("run" | "runvm" | "check" | "parse" | "lex" | "transpile" | "bench" | "build"),
        ) => c,
        _ => {
            eprintln!("{USAGE}");
            exit(2);
        }
    };
    // `build` keeps file-only source handling (Phase 1; cross targets extend it in Wave C). It
    // consumes an optional `-o <out>`; a dangling `-o`, an unrecognized trailing arg, or any extra
    // argument is a usage error (exit 2) — never a silent default-named build.
    if cmd == "build" {
        let file = match args.get(2) {
            Some(f) => f,
            None => {
                eprintln!("{USAGE}");
                exit(2);
            }
        };
        let src = read_source_file(file);
        let out = match args.get(3).map(String::as_str) {
            None => None,
            Some("-o") => match args.get(4).map(String::as_str) {
                Some(v) => Some(v),
                None => {
                    eprintln!("{USAGE}");
                    exit(2);
                }
            },
            Some(_) => {
                eprintln!("{USAGE}");
                exit(2);
            }
        };
        if args.len() > 5 {
            eprintln!("{USAGE}");
            exit(2);
        }
        match cli::cmd_build(file, &src, out) {
            Ok(text) => {
                print!("{text}");
                return;
            }
            Err(err) => {
                eprintln!("{err}");
                exit(1);
            }
        }
    }
    // Run-family: resolve the source — <file> | - (stdin) | -e/--eval <code> | -- <file>.
    let src = match cli::resolve_source(&args[2..]) {
        Some(cli::SourceSpec::File(path)) => read_source_file(&path),
        Some(cli::SourceSpec::Stdin) => read_stdin(),
        Some(cli::SourceSpec::Inline(code)) => code,
        None => {
            eprintln!("{USAGE}");
            exit(2);
        }
    };
    let result = match cmd {
        "run" => cli::cmd_run(&src),
        "runvm" => cli::cmd_runvm(&src),
        "check" => cli::cmd_check(&src),
        "parse" => cli::cmd_parse(&src),
        "lex" => cli::cmd_lex(&src),
        "transpile" => cli::cmd_transpile(&src),
        "bench" => cli::cmd_bench(&src),
        _ => unreachable!("validated above"),
    };
    match result {
        Ok(text) => print!("{text}"),
        Err(err) => {
            eprintln!("{err}");
            exit(1);
        }
    }
}

/// Read a program from a file path, exiting 1 with a message on failure.
fn read_source_file(path: &str) -> String {
    match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            exit(1);
        }
    }
}

/// Read a program from standard input, exiting 1 with a message on failure.
fn read_stdin() -> String {
    use std::io::Read;
    let mut s = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut s) {
        eprintln!("cannot read stdin: {e}");
        exit(1);
    }
    s
}
