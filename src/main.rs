//! Phorge CLI: `phg <run|runvm|check|parse|lex|transpile|disasm|bench|build|vendor|serve|explain>
//! <file>`. Thin dispatcher over the testable `phorge::cli` module.
#![forbid(unsafe_code)]

use std::process::exit;

use phorge::{cli, loader};

const USAGE: &str =
    "usage: phg <run|runvm|check|parse|lex|transpile|disasm|bench|build|vendor|serve|explain> \
                     <file | - | -e code> [-o out]   (phg -h for help, -v for version)";

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
            c @ ("run" | "runvm" | "check" | "parse" | "lex" | "transpile" | "disasm" | "bench"
            | "build" | "vendor" | "serve" | "explain"),
        ) => c,
        _ => {
            eprintln!("{USAGE}");
            exit(2);
        }
    };
    // Per-command help: `phg <cmd> -h|--help` prints command-specific help and exits 0.
    if args[2..].iter().any(|a| a == "-h" || a == "--help") {
        print!("{}", cli::help_for(cmd));
        return;
    }
    // `explain <CODE>` takes a diagnostic code, not a program source — handle it before the
    // source-resolving run-family path.
    if cmd == "explain" {
        let code = match args.get(2) {
            Some(c) => c,
            None => {
                eprintln!("usage: phg explain <CODE>");
                exit(2);
            }
        };
        match cli::cmd_explain(code) {
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
    // `vendor [project-dir | phorge.toml]` resolves a project (not a program source) and fetches its
    // git dependencies — the only network-touching command. Defaults to the current directory; any
    // extra argument is a usage error.
    if cmd == "vendor" {
        let arg = args.get(2).map(String::as_str).unwrap_or(".");
        if args.len() > 3 {
            eprintln!("{USAGE}");
            exit(2);
        }
        match cli::cmd_vendor(arg) {
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
        // Flags after `<file>`: optional -o <out>, optional (--target <triple> | --all), mutually
        // exclusive. --sign is reserved for Phase 3; unknown flags / extra args → usage, exit 2.
        let mut out: Option<&str> = None;
        let mut target: Option<&str> = None;
        let mut all = false;
        let mut i = 3;
        while i < args.len() {
            match args[i].as_str() {
                "-o" => {
                    out = Some(args.get(i + 1).map(String::as_str).unwrap_or_else(|| {
                        eprintln!("{USAGE}");
                        exit(2);
                    }));
                    i += 2;
                }
                "--target" => {
                    target = Some(args.get(i + 1).map(String::as_str).unwrap_or_else(|| {
                        eprintln!("{USAGE}");
                        exit(2);
                    }));
                    i += 2;
                }
                "--all" => {
                    all = true;
                    i += 1;
                }
                "--sign" => {
                    eprintln!("signing is Phase 3");
                    exit(2);
                }
                _ => {
                    eprintln!("{USAGE}");
                    exit(2);
                }
            }
        }
        if all && target.is_some() {
            eprintln!("{USAGE}"); // --all and --target are mutually exclusive
            exit(2);
        }
        let res = if all {
            phorge::bundle::cross::build_all(file, &src, out)
        } else if let Some(t) = target {
            phorge::bundle::cross::build_target(file, &src, t, out)
        } else {
            cli::cmd_build(file, &src, out)
        };
        match res {
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
    // `serve <file> [--addr ADDR]` runs the blocking HTTP server. The program is loaded
    // project-aware (like `run`) and must define `respond(bytes) -> bytes`. The loop runs until the
    // process is killed; only a bind/socket error returns (exit 1). Default addr 127.0.0.1:8080.
    if cmd == "serve" {
        let mut file: Option<&str> = None;
        let mut addr = "127.0.0.1:8080";
        // Per-connection read/write timeout (GA blocker B4): default 30s; `--timeout 0` disables it.
        let mut timeout_secs: u64 = 30;
        let mut i = 2;
        while i < args.len() {
            match args[i].as_str() {
                "--addr" => {
                    addr = args.get(i + 1).map(String::as_str).unwrap_or_else(|| {
                        eprintln!("{USAGE}");
                        exit(2);
                    });
                    i += 2;
                }
                "--timeout" => {
                    timeout_secs = args
                        .get(i + 1)
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or_else(|| {
                            eprintln!("phg serve: --timeout expects a whole number of seconds");
                            exit(2);
                        });
                    i += 2;
                }
                a if !a.starts_with('-') && file.is_none() => {
                    file = Some(a);
                    i += 1;
                }
                _ => {
                    eprintln!("{USAGE}");
                    exit(2);
                }
            }
        }
        let file = file.unwrap_or_else(|| {
            eprintln!("usage: phg serve <file> [--addr 127.0.0.1:8080] [--timeout 30]");
            exit(2);
        });
        let timeout = (timeout_secs > 0).then(|| std::time::Duration::from_secs(timeout_secs));
        let unit = match loader::load(std::path::Path::new(file)) {
            Ok(u) => u,
            Err(err) => {
                eprintln!("{err}");
                exit(1);
            }
        };
        match cli::serve_program(&unit.program, &unit.diag_src, addr, timeout) {
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
    // `bench` accepts an optional `--vs-php` flag (transpile + median-time the PHP backend too).
    // Strip it before source resolution so it isn't mistaken for a file/flag.
    let bench_vs_php = cmd == "bench" && args[2..].iter().any(|a| a == "--vs-php");
    // `check --json` emits machine-readable diagnostics (LSP foothold) instead of the "OK" Text.
    let check_json = cmd == "check" && args[2..].iter().any(|a| a == "--json");
    let rest: Vec<String> = args[2..]
        .iter()
        .filter(|a| a.as_str() != "--vs-php" && a.as_str() != "--json")
        .cloned()
        .collect();
    // run/runvm/check/transpile are project-aware (M5 S2b): a <file> source is resolved through the
    // project loader — a phorge.toml walk-up triggers multi-file merge + folder=path validation;
    // otherwise loose mode (single file, `package main` only). `-e`/stdin are always loose. parse,
    // lex, disasm, and bench keep the single-file string path (they dump/measure one source).
    let result = if matches!(cmd, "run" | "runvm" | "check" | "transpile") {
        let unit = match cli::resolve_source(&rest) {
            Some(cli::SourceSpec::File(path)) => loader::load(std::path::Path::new(&path)),
            Some(cli::SourceSpec::Stdin) => loader::load_loose_src(&read_stdin()),
            Some(cli::SourceSpec::Inline(code)) => loader::load_loose_src(&code),
            None => {
                eprintln!("{USAGE}");
                exit(2);
            }
        };
        let unit = match unit {
            Ok(u) => u,
            Err(err) => {
                eprintln!("{err}");
                exit(1);
            }
        };
        match cmd {
            "run" => cli::run_program(&unit.program, &unit.diag_src),
            "runvm" => cli::runvm_program(&unit.program, &unit.diag_src),
            "check" if check_json => {
                // JSON diagnostics go to stdout regardless; exit 0 (clean) or 1 (errors present).
                let (json, had_errors) = cli::check_json_program(&unit.program);
                print!("{json}");
                exit(i32::from(had_errors));
            }
            "check" => cli::check_program(&unit.program, &unit.diag_src),
            "transpile" => cli::transpile_program(&unit.program, &unit.diag_src),
            _ => unreachable!("matched above"),
        }
    } else {
        // Source forms — <file> | - (stdin) | -e/--eval <code> | -- <file>.
        let src = match cli::resolve_source(&rest) {
            Some(cli::SourceSpec::File(path)) => read_source_file(&path),
            Some(cli::SourceSpec::Stdin) => read_stdin(),
            Some(cli::SourceSpec::Inline(code)) => code,
            None => {
                eprintln!("{USAGE}");
                exit(2);
            }
        };
        match cmd {
            "parse" => cli::cmd_parse(&src),
            "lex" => cli::cmd_lex(&src),
            "disasm" => cli::cmd_disasm(&src),
            "bench" if bench_vs_php => cli::cmd_bench_vs_php(&src),
            "bench" => cli::cmd_bench(&src),
            _ => unreachable!("validated above"),
        }
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
