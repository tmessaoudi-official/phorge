//! CLI pipeline helpers, kept in the library so they are unit-testable without
//! spawning the binary. `main.rs` is a thin dispatcher over these. Each command
//! is `fn(&str) -> Result<String, String>`: `Ok` is text to print verbatim
//! (newline-terminated where appropriate), `Err` is a rendered error message.

use std::time::{Duration, Instant};

use crate::ast::Program;
use crate::checker::check;
use crate::chunk::{BytecodeProgram, Chunk, Op};
use crate::compiler::compile;
use crate::interpreter::interpret;
use crate::lexer::lex;
use crate::mem;
use crate::parser::Parser;
use crate::vm::Vm;

/// The `--version` line: `phorge <version>` (from `CARGO_PKG_VERSION`).
pub fn version_line() -> String {
    format!("phorge {}", env!("CARGO_PKG_VERSION"))
}

/// The `--help` text: version banner + commands + source forms + options.
pub fn help_text() -> String {
    format!(
        "{version}\n\
         usage:\n  \
         phorge <command> <source> [options]\n\n\
         commands:\n  \
         run        interpret the program (tree-walking)\n  \
         runvm      run the program on the bytecode VM\n  \
         check      type-check only\n  \
         parse      print the AST\n  \
         lex        print the token stream\n  \
         transpile  emit PHP\n  \
         disasm     print the compiled bytecode\n  \
         bench      benchmark run vs runvm (time + memory)\n  \
         build      compile to a standalone executable (-o <out>)\n  \
         explain    explain a diagnostic code (e.g. phorge explain E-UNKNOWN-IDENT)\n\n\
         source:\n  \
         <file>     read the program from a file\n  \
         -          read the program from stdin\n  \
         -e <code>  run an inline program (alias: --eval)\n  \
         --         treat the next argument as a file path (even if it starts with '-')\n\n\
         options:\n  \
         -h, --help     print this help and exit\n  \
         -v, --version  print the version and exit\n",
        version = version_line()
    )
}

/// Per-command help: a one-line description, the source/flag forms, and 1–2 worked examples.
/// An unknown command falls back to the top-level [`help_text`].
pub fn help_for(cmd: &str) -> String {
    let body = match cmd {
        "run" => {
            "run — interpret the program with the tree-walking interpreter.\n\n\
                  usage:\n  phorge run <file | - | -e code> [--]\n\n\
                  examples:\n  \
                  phorge run hello.phg\n  \
                  phorge run -e 'function main() { console.println(\"hi\"); }'\n  \
                  echo 'function main(){console.println(\"hi\");}' | phorge run -\n"
        }
        "runvm" => {
            "runvm — run the program on the bytecode VM (byte-identical to `run`).\n\n\
                    usage:\n  phorge runvm <file | - | -e code>\n\n\
                    examples:\n  \
                    phorge runvm hello.phg\n  \
                    phorge runvm -e 'function main() { console.println(\"{2 + 2}\"); }'\n"
        }
        "check" => {
            "check — type-check only; print OK or the type errors, run nothing.\n\n\
                    usage:\n  phorge check <file | - | -e code>\n\n\
                    examples:\n  \
                    phorge check src.phg\n"
        }
        "parse" => {
            "parse — print the parsed AST (no type-check).\n\n\
                    usage:\n  phorge parse <file | - | -e code>\n\n\
                    examples:\n  \
                    phorge parse src.phg\n"
        }
        "lex" => {
            "lex — print the token stream with positions.\n\n\
                  usage:\n  phorge lex <file | - | -e code>\n\n\
                  examples:\n  \
                  phorge lex -e 'var x = 1;'\n"
        }
        "transpile" => {
            "transpile — emit idiomatic PHP for the program.\n\n\
                        usage:\n  phorge transpile <file | - | -e code>\n\n\
                        examples:\n  \
                        phorge transpile src.phg\n"
        }
        "disasm" => {
            "disasm — print the compiled bytecode the VM will execute.\n\n\
                     usage:\n  phorge disasm <file | - | -e code>\n\n\
                     examples:\n  \
                     phorge disasm -e 'function main() { int x = 1 + 2; }'\n"
        }
        "bench" => {
            "bench — benchmark `run` vs `runvm` (median wall-clock + memory).\n\n\
                    usage:\n  phorge bench [--vs-php] <file | - | -e code>\n\n\
                    flags:\n  \
                    --vs-php   also transpile + median-time the PHP backend (3-way comparison;\n             \
                               requires `php` on PATH; output-identity-gated)\n\n\
                    examples:\n  \
                    phorge bench examples/bench/workload.phg\n  \
                    phorge bench --vs-php examples/bench/workload.phg\n"
        }
        "build" => {
            "build — compile to a standalone executable (embeds the program source).\n\n\
                    usage:\n  phorge build <file> [-o out] [--target triple | --all]\n\n\
                    examples:\n  \
                    phorge build app.phg\n  \
                    phorge build app.phg -o dist/app\n  \
                    phorge build app.phg --target x86_64-unknown-linux-musl\n"
        }
        "explain" => {
            "explain — print the explanation for a diagnostic code.\n\n\
                      usage:\n  phorge explain <CODE>\n\n\
                      examples:\n  \
                      phorge explain E-UNKNOWN-IDENT\n"
        }
        _ => return help_text(),
    };
    format!("{}\n{body}", version_line())
}

/// The prose explanation for a diagnostic `code`, or `None` if the code is unknown. The codes are
/// the stable identifiers carried by [`crate::diagnostic::Diagnostic::code`] and shown in `[…]`
/// beneath a rendered error.
pub fn explain_text(code: &str) -> Option<String> {
    let body = match code {
        "E-UNKNOWN-IDENT" => {
            "E-UNKNOWN-IDENT — a name was used that is not in scope.\n\n\
             Phorge resolves identifiers lexically: block-scope locals (including `var` bindings\n\
             and `for` loop variables), parameters, top-level functions, and — inside a method —\n\
             the current class's fields. A typo or an out-of-scope reference triggers this; the\n\
             diagnostic suggests the nearest in-scope name when one is close.\n"
        }
        "E-UNKNOWN-TYPE" => {
            "E-UNKNOWN-TYPE — a type name was used that is not defined.\n\n\
             Built-in types are `int`, `float`, `bool`, `string`, `List<T>`, `Map<K,V>`, `Set<T>`.\n\
             User types come from `class`, `enum`, and `type` alias declarations. Check the\n\
             spelling and that the declaration is present.\n"
        }
        "E-INFER-NULL" => {
            "E-INFER-NULL — `var` cannot infer a type from `null` alone.\n\n\
             `null` has no element type on its own, so `var x = null;` is rejected. Annotate the\n\
             optional instead, e.g. `int? x = null;`.\n"
        }
        "E-ALIAS-CYCLE" => {
            "E-ALIAS-CYCLE — a `type` alias refers to itself.\n\n\
             `type A = B; type B = A;` has no underlying type. Break the cycle so every alias\n\
             bottoms out at a built-in, class, or enum type.\n"
        }
        "E-RANGE-TYPE" => {
            "E-RANGE-TYPE — a range bound is not an `int`.\n\n\
             Both bounds of `a..b` / `a..=b` must be `int`; the range materializes to a\n\
             `List<int>` (its role this slice is `for (int i in 0..n)`). Use integer bounds, or\n\
             build a `List` explicitly if you need other element types.\n"
        }
        "E-NO-PACKAGE" => {
            "E-NO-PACKAGE — a file has no `package` declaration.\n\n\
             Everything is namespaced (\"nothing in the wind\"): every file must declare its package\n\
             as its first line, never inferred. A runnable program declares `package main;` (the\n\
             reserved entry); library code declares a dotted path like `package app.util;`.\n"
        }
        "E-RESERVED-PACKAGE" => {
            "E-RESERVED-PACKAGE — a user file claimed a `core` package root.\n\n\
             The `core.` root is reserved for the standard library (`core.console`, `core.math`,\n\
             `core.file`, …), like a built-in type name. Root your own packages elsewhere, e.g.\n\
             `package app;` or `package app.util;`.\n"
        }
        "E-SHADOW-IMPORT" => {
            "E-SHADOW-IMPORT — a local binding shadows an imported module qualifier.\n\n\
             Everything is namespaced (\"nothing in the wind\"): after `import core.console;` the\n\
             name `console` is a module qualifier, so a value binding (variable, parameter, loop or\n\
             match binding) of the same name would make `console.x()` ambiguous — the run backends\n\
             would read a method call, the transpiler a native. Rename the binding, or drop the\n\
             matching import.\n"
        }
        "E-OPT-ASSIGN" => {
            "E-OPT-ASSIGN — an optional `T?` was used where a non-optional `T` is required.\n\n\
             A non-optional value can never be `null`, so a `T?` cannot flow into a `T` binding,\n\
             parameter, field, or return without handling absence first. Unwrap it with `??`\n\
             (default), `?.` (safe access), `if (var x = opt) { … }`, or `opt!` (checked).\n"
        }
        "E-OPT-USE" => {
            "E-OPT-USE — a plain `.field` / `.method()` was used on an optional `T?` receiver.\n\n\
             The receiver could be `null`, so a plain member access risks a null dereference. Use\n\
             `?.` for null-safe access (the whole access yields `null` when the receiver is null),\n\
             or first narrow the optional with `if (var x = opt) { … }` or `opt!` (checked).\n"
        }
        "E-IF-LET-TYPE" => {
            "E-IF-LET-TYPE — `if (var x = …)` was given a non-optional scrutinee.\n\n\
             The if-let form narrows an optional `T?` to its non-null inner `T`, binding it inside\n\
             the then-block. A scrutinee that is already non-optional has nothing to narrow — use a\n\
             plain `if (cond)` for a boolean test, or make the scrutinee a `T?`.\n"
        }
        "E-OPT-UNWRAP" => {
            "E-OPT-UNWRAP — force-unwrap `!` was applied to a non-optional value.\n\n\
             `opt!` asserts that an optional `T?` is non-null and unwraps it to `T` (faulting at\n\
             runtime if it is null). A value that is already a non-optional `T` has nothing to\n\
             unwrap — remove the `!`.\n"
        }
        "W-FORCE-UNWRAP" => {
            "W-FORCE-UNWRAP — a force-unwrap `!` may fault at runtime (lint).\n\n\
             `opt!` aborts the program if the optional is null. This is a deliberate guardrail: it\n\
             flags every `!` so you can prefer a total alternative — `??` (default value), `?.`\n\
             (safe access), or `if (var x = opt) { … }` (narrow) — where null is a real possibility.\n"
        }
        _ => return None,
    };
    Some(body.to_string())
}

/// `explain <code>`: print the explanation for a diagnostic code, or error on an unknown one.
pub fn cmd_explain(code: &str) -> Result<String, String> {
    explain_text(code).ok_or_else(|| {
        format!(
            "unknown diagnostic code `{code}` \
             (known: E-NO-PACKAGE, E-RESERVED-PACKAGE, E-UNKNOWN-IDENT, E-UNKNOWN-TYPE, E-INFER-NULL, E-ALIAS-CYCLE, E-RANGE-TYPE, E-OPT-ASSIGN, E-OPT-USE, E-IF-LET-TYPE, E-OPT-UNWRAP, W-FORCE-UNWRAP)"
        )
    })
}

/// Where a command reads its program from, resolved from the args after the subcommand.
#[derive(Debug, PartialEq, Eq)]
pub enum SourceSpec {
    /// Read the program from this file path.
    File(String),
    /// Read the program from standard input.
    Stdin,
    /// Run this inline program text directly.
    Inline(String),
}

/// Resolve the program source from the args following the subcommand (`args[2..]`):
/// `<file>` | `-` (stdin) | `-e <code>` / `--eval <code>` | `-- <file>`. Returns `None` on a usage
/// error (missing source, dangling `-e`, an unknown leading-`-` arg, or extra positionals) — the
/// caller prints usage and exits 2.
pub fn resolve_source(rest: &[String]) -> Option<SourceSpec> {
    match rest {
        [flag, code] if flag == "-e" || flag == "--eval" => Some(SourceSpec::Inline(code.clone())),
        [sep, path] if sep == "--" => Some(SourceSpec::File(path.clone())),
        [one] if one == "-" => Some(SourceSpec::Stdin),
        [one] if !one.starts_with('-') => Some(SourceSpec::File(one.clone())),
        _ => None,
    }
}

/// Run a pipeline closure on a worker thread with a large (256 MB) stack. The lexer is iterative,
/// but the parser, checker, compiler, and tree-walking interpreter all recurse on the native stack
/// in proportion to expression/call nesting. A generous, *known* stack makes the explicit depth
/// limits (`limits::MAX_NEST_DEPTH`, `limits::MAX_CALL_DEPTH`) — not Rust's ambient frame budget —
/// the thing that bounds recursion, so adversarial-but-bounded input faults cleanly instead of
/// aborting, identically whether called from the CLI's main thread or a 2 MB test thread.
fn on_deep_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(256 * 1024 * 1024)
            .spawn_scoped(s, f)
            .expect("spawn pipeline worker thread")
            .join()
            .expect("pipeline worker thread panicked")
    })
}

/// lex + parse, rendering the stage error to a single line. Every stage now returns a unified
/// [`crate::diagnostic::Diagnostic`] that renders itself (stage prefix + position), so the CLI
/// just calls `to_string()` rather than hand-formatting per stage.
fn lex_parse(src: &str) -> Result<Program, String> {
    let tokens = lex(src).map_err(|e| e.render(src))?;
    Parser::new(tokens)
        .parse_program()
        .map_err(|e| e.render(src))
}

/// Type-check + de-alias an already-parsed program (the gate, minus lex/parse). De-aliases so every
/// backend sees alias-free types (aliases are front-end sugar; the checker validated them, including
/// cycles + built-in shadowing). Non-fatal warnings (the lint channel, M3 S2.5) render to stderr and
/// never gate the build. `diag_src` is the source used to render error carets — the single file for a
/// loose program, or `""` for a merged multi-file unit (where no single source aligns, so diagnostics
/// print message + position without a source line).
pub fn check_and_expand(prog: &Program, diag_src: &str) -> Result<Program, String> {
    match check(prog) {
        Ok(warnings) => {
            for w in &warnings {
                eprintln!("warning: {}", w.render(diag_src));
            }
            Ok(crate::checker::expand_aliases(prog))
        }
        Err(errs) => {
            let lines: Vec<String> = errs.iter().map(|e| e.render(diag_src)).collect();
            Err(lines.join("\n"))
        }
    }
}

/// lex + parse + type-check (the gate). Renders every type error, one per line.
fn parse_checked(src: &str) -> Result<Program, String> {
    let prog = lex_parse(src)?;
    check_and_expand(&prog, src)
}

/// `run`: lex -> parse -> check (gate) -> interpret -> captured stdout.
pub fn cmd_run(src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        interpret(&prog).map_err(|e| e.to_string())
    })
}

/// `runvm`: lex -> parse -> check (gate) -> compile to bytecode -> VM -> captured stdout.
/// The bytecode backend; must produce byte-identical output to `cmd_run` (differential).
pub fn cmd_runvm(src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        let program = compile(&prog).map_err(|e| e.to_string())?;
        Vm::new(&program).run().map_err(|e| e.to_string())
    })
}

/// `check`: lex -> parse -> check; report success or the type errors.
pub fn cmd_check(src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        parse_checked(src)?;
        Ok("OK (type-checks clean)\n".to_string())
    })
}

// --- Program-taking runners (M5 S2b) -----------------------------------------------------------
// The project loader (`crate::loader`) resolves a file path to a single, possibly multi-file-merged
// `Program`; these run/check/transpile it. They mirror the `cmd_*(&str)` pipelines exactly (same
// check -> de-alias -> backend), so a loose single-file program routed through `loader` produces
// byte-identical output. `diag_src` carries the source for error carets (`""` for a merged unit).

/// `run` on an already-loaded program (interpreter backend).
pub fn run_program(prog: &Program, diag_src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let checked = check_and_expand(prog, diag_src)?;
        interpret(&checked).map_err(|e| e.to_string())
    })
}

/// `runvm` on an already-loaded program (bytecode + VM backend).
pub fn runvm_program(prog: &Program, diag_src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let checked = check_and_expand(prog, diag_src)?;
        let program = compile(&checked).map_err(|e| e.to_string())?;
        Vm::new(&program).run().map_err(|e| e.to_string())
    })
}

/// `check` on an already-loaded program.
pub fn check_program(prog: &Program, diag_src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        check_and_expand(prog, diag_src)?;
        Ok("OK (type-checks clean)\n".to_string())
    })
}

/// `transpile` on an already-loaded program (emit PHP). Multi-namespace emission for a multi-package
/// project is S2c; S2b emits the existing flat form (correct for `package main` / single-package).
pub fn transpile_program(prog: &Program, diag_src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let checked = check_and_expand(prog, diag_src)?;
        crate::transpile::emit(&checked)
    })
}

/// Build a standalone executable for the host from `src`. `input_path` names the source (used to
/// derive the default output name); `out_path` overrides it. Validates the program first (never emits
/// a broken binary), then delegates to `bundle::cross::build_host`, which reuses this phorge binary as
/// the stub and embeds `src` as a `.phorge` section. Returns a one-line success message.
pub fn cmd_build(input_path: &str, src: &str, out_path: Option<&str>) -> Result<String, String> {
    cmd_check(src)?; // validate; emit nothing on failure
    let out = match out_path {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            let stem = std::path::Path::new(input_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| format!("cannot derive output name from {input_path}"))?;
            std::path::PathBuf::from(stem)
        }
    };
    crate::bundle::cross::build_host(src, &out)
}

/// `parse`: lex -> parse; dump the AST.
pub fn cmd_parse(src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let prog = lex_parse(src)?;
        Ok(format!("{prog:#?}\n"))
    })
}

/// `lex`: dump the token stream.
pub fn cmd_lex(src: &str) -> Result<String, String> {
    let tokens = lex(src).map_err(|e| e.to_string())?;
    let mut out = String::new();
    for t in tokens {
        out.push_str(&format!("{:?} @ {}:{}\n", t.kind, t.span.line, t.span.col));
    }
    Ok(out)
}

/// `transpile`: lex -> parse -> check (gate) -> emit PHP source.
pub fn cmd_transpile(src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        crate::transpile::emit(&prog)
    })
}

/// `disasm`: lex -> parse -> check (gate) -> compile -> dump the bytecode the VM will execute.
/// A read-only window onto the backend: per-function instruction listings and the program-level
/// descriptor tables. The op mnemonic is `Op`'s own `Debug`, *not* a hand-written match — so a new
/// `Op` variant appears here automatically with no second match surface to drift out of lockstep
/// (see memory `op-variant-match-coupling`); the per-op annotation is display-only with a `_`
/// fall-through, so an un-annotated new op simply shows no comment rather than failing to compile.
pub fn cmd_disasm(src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        let program = compile(&prog).map_err(|e| e.to_string())?;
        Ok(disasm_program(&program))
    })
}

/// Resolve a human-readable annotation for an index-carrying op (the value a `Const` loads, the
/// callee of a `Call`, the field/method/variant/class a member op names). Display-only: the `_`
/// arm covers every op that needs no comment, so this never has to track the full `Op` set.
fn annotate(op: &Op, chunk: &Chunk, p: &BytecodeProgram) -> Option<String> {
    match op {
        Op::Const(i) => chunk.consts.get(*i).map(|v| format!("{v:?}")),
        Op::Call(idx) => p
            .functions
            .get(*idx)
            .map(|f| format!("-> {}/{}", f.name, f.arity)),
        Op::GetField(i) => p.names.get(*i).map(|n| format!(".{n}")),
        Op::CallMethod(i, argc) => p.names.get(*i).map(|n| format!(".{n}(argc={argc})")),
        Op::CallNative(i, argc) => crate::native::registry()
            .get(*i)
            .map(|n| format!("-> {}.{}(argc={argc})", n.module, n.name)),
        Op::MakeEnum(i) | Op::MatchTag(i) => p
            .enum_descs
            .get(*i)
            .map(|d| format!("{}::{}", d.ty, d.variant)),
        Op::GetEnumField(i) => Some(format!("payload #{i}")),
        Op::MakeInstance(i) => p.class_descs.get(*i).map(|d| d.class.clone()),
        _ => None,
    }
}

/// Format a whole [`BytecodeProgram`] as a disassembly listing. Descriptor tables are emitted only
/// when non-empty; the method table is sorted (HashMap iteration order is non-deterministic —
/// invariant #8) so the output is stable across runs.
fn disasm_program(p: &BytecodeProgram) -> String {
    let mut out = format!(
        "phorge disasm — {} function(s), main = #{}\n",
        p.functions.len(),
        p.main
    );
    if !p.enum_descs.is_empty() {
        out.push_str("\nenum descriptors:\n");
        for (i, d) in p.enum_descs.iter().enumerate() {
            out.push_str(&format!("  #{i} {}::{}/{}\n", d.ty, d.variant, d.arity));
        }
    }
    if !p.class_descs.is_empty() {
        out.push_str("\nclass descriptors:\n");
        for (i, d) in p.class_descs.iter().enumerate() {
            out.push_str(&format!(
                "  #{i} {} {{ {} }}\n",
                d.class,
                d.fields.join(", ")
            ));
        }
    }
    if !p.methods.is_empty() {
        out.push_str("\nmethods:\n");
        let mut entries: Vec<_> = p.methods.iter().collect();
        entries.sort();
        for ((class, name), idx) in entries {
            out.push_str(&format!("  {class}::{name} -> #{idx}\n"));
        }
    }
    for (fi, f) in p.functions.iter().enumerate() {
        out.push_str(&format!("\nfn #{fi} {}/{}:\n", f.name, f.arity));
        for (ip, op) in f.chunk.code.iter().enumerate() {
            let line = f.chunk.lines.get(ip).copied().unwrap_or(0);
            match annotate(op, &f.chunk, p) {
                Some(a) => out.push_str(&format!("  {ip:>4}  L{line:<4} {op:?}  ; {a}\n")),
                None => out.push_str(&format!("  {ip:>4}  L{line:<4} {op:?}\n")),
            }
        }
    }
    out
}

/// Default sample count for `phorge bench`. Odd, so the median is a real observed sample rather
/// than an average of two; large enough to damp scheduler jitter on the small M2 corpus without
/// making the CLI feel slow.
const BENCH_DEFAULT_ITERS: usize = 101;

/// `bench`: *measure* the M2 thesis ("the VM executes faster than the tree-walker") instead of
/// asserting it. Parses+checks once, then reports median-of-N wall-clock for the front-end
/// (parse+check), the one-time bytecode compile, and each backend's execution phase, plus a
/// speedup verdict. Establishes the baseline that turns every later perf claim (Copy-on-`Op`,
/// deep-copy elimination, hot-path micro-perf) from Speculative into Verified — no perf-motivated
/// change should ship without a before/after number from this harness.
pub fn cmd_bench(src: &str) -> Result<String, String> {
    bench_report(src, BENCH_DEFAULT_ITERS)
}

/// `bench --vs-php`: the standard bench report plus a transpile-and-time-PHP comparison (Track D).
pub fn cmd_bench_vs_php(src: &str) -> Result<String, String> {
    bench_report_opts(src, BENCH_DEFAULT_ITERS, true)
}

/// `php --version`'s first line, or `None` if `php` is not on `PATH`. Used to gate + label the
/// `--vs-php` comparison.
fn php_version_line() -> Option<String> {
    let out = std::process::Command::new("php")
        .arg("--version")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or("php")
            .to_string(),
    )
}

/// Transpile `prog` to PHP, gate its output against `expected` (the Phorge backends' shared output),
/// then median-time `php <file>`. Returns a report section comparing PHP to the faster Phorge backend
/// (`tw`/`vm` medians), or a graceful note when `php` is absent or the transpiled output diverges.
/// Each sample spawns a `php` process — that cost is part of what's measured and is called out.
fn php_bench_section(
    prog: &Program,
    iters: usize,
    expected: &str,
    tw: Duration,
    vm: Duration,
) -> String {
    let Some(ver) = php_version_line() else {
        return "\nvs PHP: `php` not on PATH — skipping (install php to enable --vs-php)\n"
            .to_string();
    };
    let php_src = match crate::transpile::emit(prog) {
        Ok(s) => s,
        Err(e) => return format!("\nvs PHP: transpile failed ({e}) — skipping\n"),
    };
    let path = std::env::temp_dir().join(format!("phorge_bench_{}.php", std::process::id()));
    if std::fs::write(&path, &php_src).is_err() {
        return "\nvs PHP: could not write temp file — skipping\n".to_string();
    }
    let run_php = || -> Result<String, String> {
        let o = std::process::Command::new("php")
            .arg(&path)
            .output()
            .map_err(|e| e.to_string())?;
        if !o.status.success() {
            return Err(format!(
                "php exited {}: {}",
                o.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&o.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let section = match run_php() {
        Err(e) => format!("\nvs PHP: run failed ({e}) — skipping\n"),
        // Output-identity gate — the same parity contract used between the Phorge backends. A
        // divergence is a transpile-bug report, not a timing result.
        Ok(out) if out != expected => format!(
            "\nvs PHP: transpiled output differs from Phorge ({} vs {} bytes) — skipping \
             (transpile divergence, not a timing result)\n",
            out.len(),
            expected.len()
        ),
        Ok(_) => match median_of(iters, run_php) {
            Err(e) => format!("\nvs PHP: timing failed ({e})\n"),
            Ok(php) => {
                let mut s = format!("\nvs PHP — {ver}\n");
                s.push_str(&format!(
                    "  php run       {}  (spawns a process per sample)\n",
                    fmt_dur(php)
                ));
                let best = tw.min(vm);
                let best_name = if vm <= tw { "vm" } else { "tree-walk" };
                let (a, b) = (best.as_nanos(), php.as_nanos());
                if a > 0 && b > 0 {
                    if a <= b {
                        s.push_str(&format!(
                            "  winner: Phorge ({best_name}) — {:.2}× faster than PHP ({} → {})\n",
                            b as f64 / a as f64,
                            fmt_dur(php),
                            fmt_dur(best)
                        ));
                    } else {
                        s.push_str(&format!(
                            "  winner: PHP — {:.2}× faster than Phorge ({best_name}) ({} → {})\n",
                            a as f64 / b as f64,
                            fmt_dur(best),
                            fmt_dur(php)
                        ));
                    }
                }
                s.push_str(
                    "  note: PHP timing includes process spawn and depends on opcache/JIT (php.ini)\n",
                );
                s
            }
        },
    };
    let _ = std::fs::remove_file(&path);
    section
}

/// Median wall-clock of `f` over `iters` samples after one untimed warmup. Generic over the
/// closure's `Ok` value so the same path times the interpreter (`String`), the VM (`String`), and
/// the compiler (`BytecodeProgram`). Propagates the first error — a faulting program can't be
/// benchmarked. The warmup pays one-time allocation/cache costs outside the measured window.
fn median_of<T>(
    iters: usize,
    mut f: impl FnMut() -> Result<T, String>,
) -> Result<Duration, String> {
    f()?; // warmup (untimed)
    let mut samples: Vec<Duration> = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        f()?;
        samples.push(t0.elapsed());
    }
    samples.sort_unstable();
    Ok(samples[samples.len() / 2])
}

/// Adaptive duration rendering (ns / µs / ms) so a fast and a slow stage stay legible in the same
/// report instead of a fixed unit truncating one of them to `0.000`.
fn fmt_dur(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns < 1_000 {
        format!("{ns} ns")
    } else if ns < 1_000_000 {
        format!("{:.3} µs", ns as f64 / 1_000.0)
    } else {
        format!("{:.3} ms", ns as f64 / 1_000_000.0)
    }
}

/// Peak resident-memory *growth* (KiB) a single run of `f` causes: rewind the kernel high-water
/// mark, sample the current RSS, run `f` once, then read the new peak and subtract the baseline.
/// Resetting the mark per phase makes the number baseline-independent, so the tree-walker and VM
/// stay comparable even though they execute sequentially in one process (and glibc rarely returns
/// freed pages to the OS, so a lifetime peak would unfairly charge each later phase for the
/// earlier ones). `None` when `/proc` is unavailable (non-Linux). One run is enough — peak memory
/// is deterministic, so there's nothing to median. Propagates a faulting program's error.
fn peak_growth_of<T>(mut f: impl FnMut() -> Result<T, String>) -> Result<Option<u64>, String> {
    mem::reset_peak_rss();
    let before = mem::current_rss_kb();
    f()?;
    let peak = mem::peak_rss_kb();
    // `saturating_sub`: if the peak somehow reads below the baseline (sampling race), report 0
    // growth rather than underflowing.
    Ok(match (before, peak) {
        (Some(b), Some(p)) => Some(p.saturating_sub(b)),
        _ => None,
    })
}

/// Render an optional KiB measurement adaptively (`KiB` / `MiB`), or `n/a` when unavailable.
fn fmt_kb(kb: Option<u64>) -> String {
    match kb {
        None => "n/a".to_string(),
        Some(k) if k < 1024 => format!("{k} KiB"),
        Some(k) => format!("{:.2} MiB", k as f64 / 1024.0),
    }
}

/// The bench engine (separated from [`cmd_bench`] so tests can pass a small `iters`). Runs on the
/// deep-stack worker like every other pipeline command.
fn bench_report(src: &str, iters: usize) -> Result<String, String> {
    bench_report_opts(src, iters, false)
}

/// Bench engine with an opt-in PHP comparison (`--vs-php`, Track D). `vs_php` transpiles the program,
/// gates its PHP output against the Phorge backends' output, and median-times `php <file>`.
fn bench_report_opts(src: &str, iters: usize, vs_php: bool) -> Result<String, String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        let program = compile(&prog).map_err(|e| e.to_string())?;

        // Cold memory probe — measured *first*, before the parity gate and timing loops warm the
        // allocator. Peak-RSS growth is only meaningful from a cold heap: once glibc has mapped
        // pages it almost never returns them to the OS, so a post-warmup or sequential
        // per-backend figure reads ~0 and misleads. One honest cold-run number, plus the process
        // peak below, is the defensible memory signal (full per-backend attribution would need a
        // fresh process per backend — out of scope here).
        let cold_alloc = peak_growth_of(|| interpret(&prog).map_err(|e| e.to_string()))?;

        // Output-identity gate: comparing the speed of two backends that *disagree* is
        // meaningless. This is the differential harness's parity contract, enforced here at run
        // time before any timing — if it ever fails, the divergence (not the timing) is the news.
        let tw_out = interpret(&prog).map_err(|e| e.to_string())?;
        let vm_out = Vm::new(&program).run().map_err(|e| e.to_string())?;
        if tw_out != vm_out {
            return Err(format!(
                "bench aborted: backends disagree — tree-walk produced {} bytes, vm {} bytes; \
                 fix parity (see the differential harness) before benchmarking",
                tw_out.len(),
                vm_out.len()
            ));
        }

        let front = median_of(iters, || parse_checked(src))?;
        let comp = median_of(iters, || compile(&prog).map_err(|e| e.to_string()))?;
        let tw = median_of(iters, || interpret(&prog).map_err(|e| e.to_string()))?;
        let vm = median_of(iters, || Vm::new(&program).run().map_err(|e| e.to_string()))?;

        // Branch on integer nanos (no float equality); convert to f64 only for the ratio display.
        let tw_ns = tw.as_nanos();
        let vm_ns = vm.as_nanos();
        let verdict = if tw_ns == 0 || vm_ns == 0 {
            "verdict: backend execution too fast to measure at this sample size — \
             use a heavier corpus"
                .to_string()
        } else if vm_ns <= tw_ns {
            format!(
                "verdict: vm run is {:.2}× faster than tree-walk run ({} → {})",
                tw_ns as f64 / vm_ns as f64,
                fmt_dur(tw),
                fmt_dur(vm)
            )
        } else {
            format!(
                "verdict: tree-walk run is {:.2}× faster than vm run ({} → {})",
                vm_ns as f64 / tw_ns as f64,
                fmt_dur(vm),
                fmt_dur(tw)
            )
        };

        let mut out = String::new();
        out.push_str(&format!(
            "phorge bench — median of {iters} (warmup 1, std Instant)\n"
        ));
        out.push_str(&format!(
            "output: {} bytes, identical on both backends\n\n",
            tw_out.len()
        ));
        out.push_str(&format!("  parse+check   {}\n", fmt_dur(front)));
        out.push_str(&format!(
            "  compile       {}  (one-time, vm only)\n",
            fmt_dur(comp)
        ));
        out.push_str(&format!("  tree-walk run {}\n", fmt_dur(tw)));
        out.push_str(&format!("  vm run        {}\n\n", fmt_dur(vm)));
        out.push_str(&verdict);
        out.push('\n');

        // Optional PHP comparison (Track D) — appended after the Phorge verdict, before memory.
        if vs_php {
            out.push_str(&php_bench_section(&prog, iters, &tw_out, tw, vm));
        }

        // Memory (Linux /proc). The cold-run growth (captured before any warmup) is the workload's
        // own resident footprint for one execution; the process figures are the bench process's
        // lifetime high-water mark and current resident set.
        match cold_alloc {
            None => out.push_str("\nmemory: unavailable on this platform (requires Linux /proc)\n"),
            Some(g) => {
                out.push_str("\nmemory\n");
                out.push_str(&format!(
                    "  cold run      +{} RSS  (one tree-walk execution from a cold heap)\n",
                    fmt_kb(Some(g))
                ));
                out.push_str(&format!(
                    "  process peak  {}  (VmHWM)\n",
                    fmt_kb(mem::peak_rss_kb())
                ));
                out.push_str(&format!(
                    "  resident now  {}  (VmRSS)\n",
                    fmt_kb(mem::current_rss_kb())
                ));
            }
        }
        Ok(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prepend the reserved `package main;` (M5 S1: every file is packaged, never inferred) unless
    /// already declared, so the CLI command tests need no per-case package boilerplate. The segment
    /// carries no newline, so line numbers in fault diagnostics are preserved.
    fn wp(src: &str) -> String {
        if src.trim_start().starts_with("package ") {
            src.to_string()
        } else {
            format!("package main; {src}")
        }
    }

    const SAMPLE: &str = r#"package main;
import core.console;

enum Shape {
    Circle(float radius),
    Rect(float w, float h),
}

function area(Shape s) -> float {
    return match s {
        Circle(r)  => 3.14159 * r * r,
        Rect(w, h) => w * h,
    };
}

class Greeter {
    private string name;
    constructor(private string name) {}
    function greet() -> string { return "Hello {name}"; }
}

function main() {
    Greeter g = Greeter("Tak");
    console.println(g.greet());
    List<Shape> shapes = [Circle(2.0), Rect(3.0, 4.0)];
    for (Shape s in shapes) {
        console.println("area = {area(s)}");
    }
}
"#;

    fn rest(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn resolve_source_handles_all_forms() {
        assert_eq!(
            resolve_source(&rest(&["prog.phg"])),
            Some(SourceSpec::File("prog.phg".into()))
        );
        assert_eq!(resolve_source(&rest(&["-"])), Some(SourceSpec::Stdin));
        assert_eq!(
            resolve_source(&rest(&["-e", "x"])),
            Some(SourceSpec::Inline("x".into()))
        );
        assert_eq!(
            resolve_source(&rest(&["--eval", "y"])),
            Some(SourceSpec::Inline("y".into()))
        );
        // `--` lets a path start with '-'.
        assert_eq!(
            resolve_source(&rest(&["--", "-weird.phg"])),
            Some(SourceSpec::File("-weird.phg".into()))
        );
    }

    #[test]
    fn resolve_source_rejects_bad_forms() {
        assert_eq!(resolve_source(&rest(&[])), None); // missing source
        assert_eq!(resolve_source(&rest(&["-e"])), None); // -e without code
        assert_eq!(resolve_source(&rest(&["-x"])), None); // unknown flag (use -- to pass it)
        assert_eq!(resolve_source(&rest(&["a", "b"])), None); // too many positionals
    }

    #[test]
    fn run_executes_sample() {
        assert_eq!(
            cmd_run(SAMPLE).unwrap(),
            "Hello Tak\narea = 12.56636\narea = 12\n"
        );
    }

    #[test]
    fn run_reports_type_error_and_does_not_execute() {
        // `area` returns float; returning an int literal is a type error.
        let src = wp(r#"import core.console;
function area() -> float { return 1; } function main() { console.println("{area()}"); }"#);
        let err = cmd_run(&src).unwrap_err();
        assert!(err.contains("type error"), "{err}");
    }

    #[test]
    fn run_reports_runtime_error() {
        let err = cmd_run(&wp(r#"import core.console;
function main() { console.println("{1 / 0}"); }"#))
        .unwrap_err();
        assert!(err.contains("runtime error"), "{err}");
    }

    #[test]
    fn run_reports_parse_error() {
        let err = cmd_run(&wp("function main( {")).unwrap_err();
        assert!(err.contains("parse error"), "{err}");
    }

    #[test]
    fn check_passes_on_clean_program() {
        let ok = cmd_check(SAMPLE).unwrap();
        assert!(ok.contains("OK"), "{ok}");
    }

    #[test]
    fn check_fails_on_type_error() {
        let src = wp(r#"function f() -> float { return 1; } function main() {}"#);
        assert!(cmd_check(&src).unwrap_err().contains("type error"));
    }

    #[test]
    fn parse_dumps_ast() {
        let out = cmd_parse(r#"function main() {}"#).unwrap();
        assert!(out.contains("Program"), "{out}");
    }

    #[test]
    fn lex_dumps_tokens() {
        let out = cmd_lex(r#"function main() {}"#).unwrap();
        assert!(out.contains("@ 1:1"), "{out}");
    }

    #[test]
    fn cmd_transpile_emits_php_for_sample() {
        let php = cmd_transpile(SAMPLE).expect("transpile");
        assert!(php.starts_with("<?php\n"), "{php}");
        assert!(php.contains("abstract class Shape {}"), "{php}");
        assert!(
            php.contains("function __construct(private string $name) {}"),
            "{php}"
        );
    }

    #[test]
    fn cmd_transpile_rejects_ill_typed() {
        let err = cmd_transpile(&wp(r#"function main() { int x = "no"; }"#)).unwrap_err();
        assert!(err.contains("type error"), "{err}");
    }

    #[test]
    fn runvm_matches_run_on_simple_program() {
        let src = wp(r#"import core.console;
function main() { int x = 21; console.println("{x + x}"); }"#);
        assert_eq!(cmd_runvm(&src).unwrap(), cmd_run(&src).unwrap());
        assert_eq!(cmd_runvm(&src).unwrap(), "42\n");
    }

    #[test]
    fn runvm_reports_type_error_via_the_gate() {
        let err = cmd_runvm(&wp(r#"function main() { int x = "no"; }"#)).unwrap_err();
        assert!(err.contains("type error"), "{err}");
    }

    #[test]
    fn runvm_reports_runtime_error_with_prefix() {
        let err = cmd_runvm(&wp(r#"import core.console;
function main() { console.println("{1 / 0}"); }"#))
        .unwrap_err();
        assert!(err.contains("runtime error"), "{err}");
    }

    #[test]
    fn runvm_runtime_error_carries_source_line() {
        // div-by-zero in a statement on line 3. The VM now locates the fault via `Chunk.lines`
        // and renders `runtime error at 3: …`, while the canonical body ("division by zero")
        // stays intact so the differential `agree_err` oracle still classifies it identically.
        // NB: the division is *not* inside string interpolation — `split_interpolation`
        // re-lexes interpolated sub-expressions with a fresh lexer that resets to line 1, so a
        // fault inside `"{…}"` reports line 1 (a pre-existing interpolation-position limitation,
        // orthogonal to this task — see the M2 P3.5 roadmap decisions log).
        let src = wp("import core.console; function main() {\n    int z = 0;\n    int x = 1 / z;\n    console.println(\"{x}\");\n}");
        let err = cmd_runvm(&src).unwrap_err();
        assert!(err.contains("division by zero"), "{err}");
        assert!(err.starts_with("runtime error at 3:"), "{err}");
    }

    #[test]
    fn run_runtime_error_has_no_line() {
        // The tree-walking interpreter tracks no source position, so its runtime errors keep
        // the position-less `runtime error: …` form (deliberate asymmetry — documented).
        let src = wp("import core.console; function main() {\n    int z = 0;\n    int x = 1 / z;\n    console.println(\"{x}\");\n}");
        let err = cmd_run(&src).unwrap_err();
        assert!(err.starts_with("runtime error: "), "{err}");
        assert!(!err.contains(" at "), "{err}");
    }

    #[test]
    fn bench_reports_both_backends_with_identical_output() {
        // Small iteration count keeps the test fast; the report must name both backends, confirm
        // output identity (and the byte count it asserted), and end in a verdict comparing them.
        let src = wp(r#"import core.console;
function main() { int x = 21; console.println("{x + x}"); }"#);
        let out = bench_report(&src, 5).expect("bench");
        assert!(out.contains("tree-walk run"), "{out}");
        assert!(out.contains("vm run"), "{out}");
        assert!(out.contains("identical on both backends"), "{out}");
        assert!(out.contains("verdict:"), "{out}");
        // Output is "42\n" = 3 bytes — the report states the byte count it asserted identical.
        assert!(out.contains("3 bytes"), "{out}");
    }

    #[test]
    fn bench_vs_php_emits_a_php_section() {
        // `--vs-php` always emits a "vs PHP" section — either the comparison (php present) or a
        // graceful skip note (php absent). Both start with "vs PHP", so the test is host-agnostic.
        let src = wp(r#"import core.console;
function main() { int x = 21; console.println("{x + x}"); }"#);
        let out = bench_report_opts(&src, 3, true).expect("bench");
        assert!(out.contains("vs PHP"), "{out}");
        // The standard report is still present.
        assert!(out.contains("vm run"), "{out}");
    }

    #[test]
    fn bench_reports_a_memory_section() {
        // Beyond timing, the report carries a memory block. The header is printed unconditionally
        // (the per-phase numbers are present on Linux, "unavailable" elsewhere), so asserting the
        // header keeps the test platform-independent.
        let src = wp(r#"import core.console;
function main() { console.println("hi"); }"#);
        let out = bench_report(&src, 5).expect("bench");
        assert!(out.contains("memory"), "{out}");
    }

    #[test]
    fn disasm_dumps_bytecode_with_mnemonics_and_annotations() {
        // The disassembler names the function, prints the type-specialized int-add op, the native
        // call op (the migrated former `Print`), and annotates a constant load with its value.
        let out = cmd_disasm(&wp(
            r#"import core.console; function main() { int x = 1 + 2; console.println("{x}"); }"#,
        ))
        .expect("disasm");
        assert!(out.contains("fn #"), "{out}");
        assert!(out.contains("main/0"), "{out}");
        assert!(out.contains("AddI"), "{out}");
        // `console.println` lowers to `Op::CallNative`, annotated with the resolved native path.
        assert!(out.contains("CallNative"), "{out}");
        assert!(out.contains("core.console.println"), "{out}");
        // Const loads carry a `; <value>` annotation resolved from the pool.
        assert!(out.contains("Const(") && out.contains("; "), "{out}");
    }

    #[test]
    fn explain_covers_shadow_import_code() {
        // The M3 Wave 1 shadowing diagnostic is self-documenting via `phorge explain`.
        let body = explain_text("E-SHADOW-IMPORT").expect("E-SHADOW-IMPORT has an explanation");
        assert!(body.contains("module qualifier"), "{body}");
    }

    #[test]
    fn explain_covers_m5_package_codes() {
        // The M5 S1 package diagnostics are self-documenting via `phorge explain`.
        let np = explain_text("E-NO-PACKAGE").expect("E-NO-PACKAGE has an explanation");
        assert!(np.contains("package main"), "{np}");
        let rp = explain_text("E-RESERVED-PACKAGE").expect("E-RESERVED-PACKAGE has an explanation");
        assert!(rp.contains("standard library"), "{rp}");
    }

    #[test]
    fn disasm_propagates_type_error() {
        // A program that fails the gate can't be disassembled — the type error surfaces instead.
        let err = cmd_disasm(&wp(r#"function main() { int x = "no"; }"#)).unwrap_err();
        assert!(err.contains("type error"), "{err}");
    }

    #[test]
    fn bench_propagates_type_error_without_timing() {
        // A program that fails the gate can't be benchmarked — the error surfaces, no timing runs.
        let err = bench_report(&wp(r#"function main() { int x = "no"; }"#), 5).unwrap_err();
        assert!(err.contains("type error"), "{err}");
    }

    #[test]
    fn bench_default_entry_uses_101_samples() {
        // The public entry runs the default-N path end to end (smoke test of `cmd_bench`).
        let out = cmd_bench(&wp(r#"import core.console;
function main() { console.println("hi"); }"#))
        .expect("bench");
        assert!(out.starts_with("phorge bench — median of 101"), "{out}");
    }

    #[test]
    fn help_for_known_command_has_examples_and_name() {
        let h = help_for("run");
        assert!(h.contains("examples:"), "{h}");
        assert!(h.contains("phorge run"), "{h}");
    }

    #[test]
    fn help_for_unknown_command_falls_back_to_top_level() {
        assert_eq!(help_for("bogus"), help_text());
    }

    #[test]
    fn var_transpiles_to_plain_php_assignment() {
        // `var` is erased; PHP locals are untyped, so it emits a bare `$x = …;`.
        let php = cmd_transpile(&wp(
            "import core.console; function main() { var x = 1; console.println(\"{x}\"); }",
        ))
        .unwrap();
        assert!(php.contains("$x = 1;"), "{php}");
    }

    #[test]
    fn type_alias_is_erased_in_php() {
        // The alias declaration vanishes and `Count` resolves to `int` in the emitted signature.
        let php = cmd_transpile(&wp(
            "type Count = int; function tally(Count n) -> Count { return n + 1; } function main() {}",
        ))
        .unwrap();
        assert!(!php.contains("Count"), "alias leaked into PHP:\n{php}");
        assert!(php.contains("function tally(int $n): int"), "{php}");
    }

    #[test]
    fn explain_known_code_returns_paragraph_unknown_errors() {
        let ok = cmd_explain("E-UNKNOWN-IDENT").unwrap();
        assert!(ok.contains("E-UNKNOWN-IDENT"), "{ok}");
        assert!(ok.len() > 40, "explanation too short: {ok}");
        assert!(cmd_explain("E-NOPE").is_err());
    }
}
