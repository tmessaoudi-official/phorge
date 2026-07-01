//! Phorj CLI: `phg <run|runvm|check|parse|tokenize|transpile|lift|disassemble|benchmark|build|vendor|serve|explain>
//! <file>`. Thin dispatcher over the testable `phorj::cli` module.
#![forbid(unsafe_code)]

use std::process::exit;

use phorj::{cli, loader};

const USAGE: &str =
    "usage: phg <run|runvm|check|parse|tokenize|transpile|lift|disassemble|benchmark|build|vendor|serve|lsp|test|format|explain> \
                     <file | - | -e code> [-o out]   (phg -h for help, -v for version)";

fn main() {
    // Self-executing artifact: if this binary carries an embedded program, run it on the VM and
    // exit before any CLI parsing. No payload -> fall through to the normal dispatcher.
    if let Some((src, profile)) = phorj::bundle::embedded_program() {
        // M-DX S0: a shipped artifact runs at the profile baked into its container (Release by
        // default, Dev only if built with `--dev`). Set it before running so profile-gated machinery
        // reads it — secure by construction: no environment variable can flip a Release artifact.
        phorj::profile::set_active(profile);
        // A standalone built binary runs as a normal executable, so `Core.Process.args()` reads the
        // real process arguments (everything after the program name).
        phorj::native::set_process_args(std::env::args().skip(1).collect());
        // Batch-1 B: a built binary honors `main`'s `int` return as its process exit status.
        match cli::cmd_runvm_exit(&src) {
            Ok((text, code)) => {
                print!("{text}");
                exit(i32::try_from(code).unwrap_or(1));
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
    // Feature C migration tool (internal, not in USAGE): `phg rewrite-new <file>` rewrites every
    // class/enum-variant construction to `new …` in place. Handled before the run-family dispatch.
    if args.get(1).map(String::as_str) == Some("rewrite-new") {
        let path = match args.get(2) {
            Some(p) => p,
            None => {
                eprintln!("usage: phg rewrite-new <file.phg>");
                exit(2);
            }
        };
        match cli::cmd_rewrite_new(path) {
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
    let cmd = match args.get(1).map(String::as_str) {
        Some(
            c @ ("run" | "runvm" | "check" | "parse" | "tokenize" | "transpile" | "lift"
            | "disassemble" | "benchmark" | "build" | "vendor" | "serve" | "lsp" | "test"
            | "format" | "explain"),
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
    // `test [path…]` discovers and runs `test` blocks (M-Test). It takes optional file/dir paths
    // (not a single program source), so it is handled before the source-resolving run-family path.
    if cmd == "test" {
        let paths: Vec<String> = args[2..].to_vec();
        let (report, code) = cli::cmd_test(&paths);
        print!("{report}");
        exit(code as i32);
    }
    // `fmt [--check] [path… | -]` formats source (M-fmt). Like `test`, it takes paths/flags, not a
    // single program source, so it is handled before the source-resolving run-family path.
    if cmd == "format" {
        // `phg fmt -` (or `--check -`) reads stdin → formats → stdout.
        if args[2..].iter().any(|a| a == "-") {
            let src = read_stdin();
            match cli::fmt_source(&src) {
                Ok(out) => {
                    print!("{out}");
                    return;
                }
                Err(err) => {
                    eprintln!("{err}");
                    exit(2);
                }
            }
        }
        let mut check = false;
        let mut paths: Vec<String> = Vec::new();
        for a in &args[2..] {
            match a.as_str() {
                "--check" => check = true,
                other => paths.push(other.to_string()),
            }
        }
        let (report, code) = cli::cmd_fmt(&paths, check);
        print!("{report}");
        exit(code as i32);
    }
    // `vendor [project-dir | phorj.toml]` resolves a project (not a program source) and fetches its
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
        // M-DX S0: `phg build` is Release by default (secure by construction — value-exposing
        // machinery is gated off in the artifact). `--dev` opts a debug artifact in.
        let mut profile = phorj::profile::Profile::Release;
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
                "--dev" => {
                    profile = phorj::profile::Profile::Dev;
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
            phorj::bundle::cross::build_all(file, &src, out, profile)
        } else if let Some(t) = target {
            phorj::bundle::cross::build_target(file, &src, t, out, profile)
        } else {
            cli::cmd_build(file, &src, out, profile)
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
    // `lsp` runs the language server over stdio (Item D). No source file: it speaks JSON-RPC on
    // stdin/stdout for an editor client. Returns the process exit code (0 after a clean shutdown/exit).
    if cmd == "lsp" {
        match phorj::lsp::run() {
            Ok(code) => exit(code),
            Err(e) => {
                eprintln!("lsp: {e}");
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
        // `--dev` opts into the rich HTML error page on an uncaught handler fault. OFF by default:
        // production must never leak a stack trace / source (a security rule) — it returns a bare 500.
        let mut dev = false;
        // `--workers N` request concurrency (M6 W3). 0 (the sentinel) = auto = CPU cores; 1 = the
        // single-threaded path. Resolved after parsing.
        let mut workers: usize = 0;
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
                "--dev" => {
                    dev = true;
                    i += 1;
                }
                "--workers" => {
                    workers = args
                        .get(i + 1)
                        .and_then(|s| s.parse::<usize>().ok())
                        .filter(|n| *n >= 1)
                        .unwrap_or_else(|| {
                            eprintln!("phg serve: --workers expects a positive whole number");
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
            eprintln!(
                "usage: phg serve <file> [--addr 127.0.0.1:8080] [--timeout 30] [--workers N]"
            );
            exit(2);
        });
        let timeout = (timeout_secs > 0).then(|| std::time::Duration::from_secs(timeout_secs));
        // Resolve the worker count: explicit `--workers N` wins; otherwise auto = available CPU cores
        // (fall back to 1 if the platform can't report it).
        let workers = if workers >= 1 {
            workers
        } else {
            std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
        };
        let unit = match loader::load(std::path::Path::new(file)) {
            Ok(u) => u,
            Err(err) => {
                eprintln!("{err}");
                exit(1);
            }
        };
        // M-DX S0: `--dev` selects the Dev profile (rich fault pages); the default is the secure
        // Release profile (bare 500, no trace/source leak). Set it as the process profile too.
        let profile = if dev {
            phorj::profile::Profile::Dev
        } else {
            phorj::profile::Profile::Release
        };
        phorj::profile::set_active(profile);
        match cli::serve_program(
            &unit.program,
            &unit.diag_src,
            addr,
            timeout,
            profile,
            workers,
        ) {
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
    let bench_vs_php = cmd == "benchmark" && args[2..].iter().any(|a| a == "--vs-php");
    // `check --json` emits machine-readable diagnostics (LSP foothold) instead of the "OK" Text.
    let check_json = cmd == "check" && args[2..].iter().any(|a| a == "--json");
    // `benchmark --json` emits the measurements as a machine-readable object (M-DOGFOOD W9).
    let bench_json = cmd == "benchmark" && args[2..].iter().any(|a| a == "--json");
    // `run`/`runvm --dump-on-fault` (M-DX S3): on an uncaught fault, dump the faulting frame's locals
    // to stderr. Dev-only + opt-in; the interpreter produces the rich dump (see `crate::dump`).
    let dump_on_fault =
        matches!(cmd, "run" | "runvm") && args[2..].iter().any(|a| a == "--dump-on-fault");
    let rest: Vec<String> = args[2..]
        .iter()
        .filter(|a| {
            a.as_str() != "--vs-php" && a.as_str() != "--json" && a.as_str() != "--dump-on-fault"
        })
        .cloned()
        .collect();
    // run/runvm/check/transpile are project-aware (M5 S2b): a <file> source is resolved through the
    // project loader — a phorj.toml walk-up triggers multi-file merge + folder=path validation;
    // otherwise loose mode (single file, `package Main` only). `-e`/stdin are always loose. parse,
    // lex, disasm, and bench keep the single-file string path (they dump/measure one source).
    let result = if matches!(cmd, "run" | "runvm" | "check" | "transpile") {
        // Resolve the source AND the program argv (`-- a b c`); the argv feeds `Core.Process.args()`
        // and is only meaningful for run/runvm (check/transpile ignore it).
        let (spec, prog_args) = match cli::resolve_source_and_args(&rest) {
            Some(pair) => pair,
            None => {
                eprintln!("{USAGE}");
                exit(2);
            }
        };
        if matches!(cmd, "run" | "runvm") {
            phorj::native::set_process_args(prog_args);
            // M-DX S0/S3: the interactive run tool is the Dev profile; enable the value-dump when
            // `--dump-on-fault` was passed (Dev + opt-in — `dump::should_dump` re-checks the profile).
            phorj::profile::set_active(phorj::profile::Profile::Dev);
            phorj::dump::set_enabled(dump_on_fault);
        }
        let unit = match spec {
            cli::SourceSpec::File(path) => loader::load(std::path::Path::new(&path)),
            cli::SourceSpec::Stdin => loader::load_loose_src(&read_stdin()),
            cli::SourceSpec::Inline(code) => loader::load_loose_src(&code),
        };
        let unit = match unit {
            Ok(u) => u,
            Err(err) => {
                eprintln!("{err}");
                exit(1);
            }
        };
        // run/runvm carry an exit code (Batch-1 B): `main`'s `int` return becomes the process exit
        // status. Print stdout, then exit with the code (errors already exit 1 above/below).
        if matches!(cmd, "run" | "runvm") {
            let outcome = if cmd == "run" {
                cli::run_program_exit(&unit)
            } else {
                cli::runvm_program_exit(&unit)
            };
            match outcome {
                Ok((text, code)) => {
                    print!("{text}");
                    exit(i32::try_from(code).unwrap_or(1));
                }
                Err(err) => {
                    eprintln!("{err}");
                    exit(1);
                }
            }
        }
        match cmd {
            "check" if check_json => {
                // JSON diagnostics go to stdout regardless; exit 0 (clean) or 1 (errors present).
                let (json, had_errors) = cli::check_json_program(&unit.program);
                print!("{json}");
                exit(i32::from(had_errors));
            }
            "check" => cli::check_program(&unit.program, &unit.diag_src).map(|ok| {
                // In project mode, replace the bland OK with a scope summary proving the whole
                // project (every file + vendored deps) was validated — not just the entry route.
                unit.stats.map_or(ok, |s| s.summary())
            }),
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
            "tokenize" => cli::cmd_lex(&src),
            "lift" => cli::cmd_lift(&src),
            "disassemble" => cli::cmd_disasm(&src),
            "benchmark" if bench_vs_php && bench_json => cli::cmd_bench_vs_php_json(&src),
            "benchmark" if bench_vs_php => cli::cmd_bench_vs_php(&src),
            "benchmark" if bench_json => cli::cmd_bench_json(&src),
            "benchmark" => cli::cmd_bench(&src),
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
