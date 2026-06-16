//! Phorge CLI: `phorge <run|runvm|check|parse|lex|transpile|bench> <file>`. Thin dispatcher
//! over the testable `phorge::cli` module.
#![forbid(unsafe_code)]

use std::process::exit;

use phorge::cli;

const USAGE: &str = "usage: phorge <run|runvm|check|parse|lex|transpile|bench> <file>";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = match args.get(1).map(String::as_str) {
        Some(c @ ("run" | "runvm" | "check" | "parse" | "lex" | "transpile" | "bench")) => c,
        _ => {
            eprintln!("{USAGE}");
            exit(2);
        }
    };
    let file = match args.get(2) {
        Some(f) => f,
        None => {
            eprintln!("{USAGE}");
            exit(2);
        }
    };
    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {file}: {e}");
            exit(1);
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
