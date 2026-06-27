//! CLI pipeline helpers, kept in the library so they are unit-testable without
//! spawning the binary. `main.rs` is a thin dispatcher over these. Each command
//! is `fn(&str) -> Result<String, String>`: `Ok` is text to print verbatim
//! (newline-terminated where appropriate), `Err` is a rendered error message.

use crate::ast::Program;
use crate::chunk::{BytecodeProgram, Chunk, Op};
use crate::compiler::compile;
use crate::interpreter::{interpret, interpret_main};
use crate::lexer::lex;
use crate::parser::Parser;
use crate::vm::Vm;

// Self-contained command groups (M-Decomp W1.2): the `explain` diagnostic-code table and the
// `bench` profiling suite. Re-exported so callers keep referring to `cli::cmd_explain` etc.
mod bench;
mod explain;
mod fmt_cmd;
mod rewrite_new;
mod test_runner;
pub use bench::{cmd_bench, cmd_bench_vs_php};
pub use explain::{cmd_explain, explain_text};
pub use fmt_cmd::{cmd_fmt, fmt_source};
pub use rewrite_new::cmd_rewrite_new;
pub use test_runner::cmd_test;

/// The `--version` line: `phg <version>` (from `CARGO_PKG_VERSION`).
pub fn version_line() -> String {
    format!("phg {}", env!("CARGO_PKG_VERSION"))
}

/// The `--help` text: version banner + commands + source forms + options.
pub fn help_text() -> String {
    format!(
        "{version}\n\
         usage:\n  \
         phg <command> <source> [options]\n\n\
         commands:\n  \
         run        interpret the program (tree-walking)\n  \
         runvm      run the program on the bytecode VM\n  \
         check      type-check only\n  \
         parse      print the AST\n  \
         lex        print the token stream\n  \
         transpile  emit PHP\n  \
         lift       PHP -> a Phorge draft (review required; inverse of transpile)\n  \
         disasm     print the compiled bytecode\n  \
         bench      benchmark run vs runvm (time + memory)\n  \
         build      compile to a standalone executable (-o <out>)\n  \
         vendor     fetch [require] git deps into an offline vendor/ (writes phorge.lock)\n  \
         serve      serve the program over HTTP (calls respond(bytes) -> bytes per request)\n  \
         test       discover and run `test` blocks (under tests/, or a given file/dir)\n  \
         fmt        format source to canonical form (--check for CI; - for stdin)\n  \
         explain    explain a diagnostic code (e.g. phg explain E-UNKNOWN-IDENT)\n\n\
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
                  usage:\n  phg run <file | - | -e code> [--]\n\n\
                  examples:\n  \
                  phg run hello.phg\n  \
                  phg run -e 'function main() -> void { Console.println(\"hi\"); }'\n  \
                  echo 'function main()-> void {Console.println(\"hi\");}' | phg run -\n"
        }
        "runvm" => {
            "runvm — run the program on the bytecode VM (byte-identical to `run`).\n\n\
                    usage:\n  phg runvm <file | - | -e code>\n\n\
                    examples:\n  \
                    phg runvm hello.phg\n  \
                    phg runvm -e 'function main() -> void { Console.println(\"{2 + 2}\"); }'\n"
        }
        "check" => {
            "check — type-check only; print OK or the type errors, run nothing.\n\n\
                    usage:\n  phg check [--json] <file | - | -e code>\n\n\
                    flags:\n  \
                    --json   emit diagnostics as a JSON array (stage/severity/message/line/col/\n           \
                    code/hint) to stdout for editors/LSP; exit 0 if clean, 1 if errors\n\n\
                    examples:\n  \
                    phg check src.phg\n  \
                    phg check --json src.phg\n"
        }
        "parse" => {
            "parse — print the parsed AST (no type-check).\n\n\
                    usage:\n  phg parse <file | - | -e code>\n\n\
                    examples:\n  \
                    phg parse src.phg\n"
        }
        "lex" => {
            "lex — print the token stream with positions.\n\n\
                  usage:\n  phg lex <file | - | -e code>\n\n\
                  examples:\n  \
                  phg lex -e 'var x = 1;'\n"
        }
        "transpile" => {
            "transpile — emit idiomatic PHP for the program.\n\n\
                        usage:\n  phg transpile <file | - | -e code>\n\n\
                        examples:\n  \
                        phg transpile src.phg\n"
        }
        "lift" => {
            "lift — read PHP, emit a Phorge **draft** (the inverse of transpile). Best-effort and\n       \
                   REVIEW-REQUIRED: the output is a scaffold a human checks, prefixed `// lifted\n       \
                   (verify)`. Anything outside the Tier-1 subset (e.g. an `array` type, a backed enum,\n       \
                   string interpolation) is refused with a clear `lift …` error rather than guessed.\n\n\
                   usage:\n  phg lift <file.php | - | -e code>\n\n\
                   examples:\n  \
                   phg lift legacy.php\n  \
                   phg lift legacy.php > draft.phg\n"
        }
        "disasm" => {
            "disasm — print the compiled bytecode the VM will execute.\n\n\
                     usage:\n  phg disasm <file | - | -e code>\n\n\
                     examples:\n  \
                     phg disasm -e 'function main() -> void { int x = 1 + 2; }'\n"
        }
        "bench" => {
            "bench — benchmark `run` vs `runvm` (median wall-clock + memory).\n\n\
                    usage:\n  phg bench [--vs-php] <file | - | -e code>\n\n\
                    flags:\n  \
                    --vs-php   also transpile + median-time the PHP backend (3-way comparison;\n             \
                               requires `php` on PATH; output-identity-gated)\n\n\
                    examples:\n  \
                    phg bench examples/bench/workload.phg\n  \
                    phg bench --vs-php examples/bench/workload.phg\n"
        }
        "build" => {
            "build — compile to a standalone executable (embeds the program source).\n\n\
                    usage:\n  phg build <file> [-o out] [--target triple | --all]\n\n\
                    examples:\n  \
                    phg build app.phg\n  \
                    phg build app.phg -o dist/app\n  \
                    phg build app.phg --target x86_64-unknown-linux-musl\n"
        }
        "test" => {
            "test — discover and run `test \"name\" { … }` blocks on the interpreter.\n\n\
                   With no path, runs every `*.phg` under the project's `tests/` directory (the\n\
                   project root is the nearest ancestor holding a `phorge.toml`, else the current\n\
                   directory). With a path, runs that file, or every `*.phg` under that directory.\n\
                   Each test runs independently; a failing assertion (or any fault) is reported with\n\
                   its message and the test keeps going. Exit 0 iff every test passed, else 1.\n\n\
                   usage:\n  phg test [path…]\n\n\
                   examples:\n  \
                   phg test\n  \
                   phg test tests/math.phg\n  \
                   phg test tests/\n"
        }
        "fmt" => {
            "fmt — format Phorge source to canonical form (comment-preserving, meaning-preserving).\n\n\
                  Prints from the parsed AST, so formatting never changes what the program means\n\
                  (parse(fmt(x)) == parse(x)); it is idempotent, and an unparseable file is left\n\
                  untouched (its diagnostic is reported, exit 2). v1 is tidy + comment-safe (canonical\n\
                  indentation/spacing/blank-lines), no line-wrapping yet.\n\n\
                  usage:\n  phg fmt [--check] [path… | -]\n\n\
                  flags:\n  \
                  --check   report files that aren't already formatted and exit 1; write nothing (CI)\n\n\
                  paths:\n  \
                  <none>    format every *.phg under the current directory, recursively\n  \
                  <file>    format that file in place\n  \
                  <dir>     format every *.phg under that directory in place\n  \
                  -         read from stdin, write the formatted result to stdout\n\n\
                  examples:\n  \
                  phg fmt\n  \
                  phg fmt src/app.phg\n  \
                  phg fmt --check .\n  \
                  cat app.phg | phg fmt -\n"
        }
        "explain" => {
            "explain — print the explanation for a diagnostic code.\n\n\
                      usage:\n  phg explain <CODE>\n\n\
                      examples:\n  \
                      phg explain E-UNKNOWN-IDENT\n"
        }
        "vendor" => {
            "vendor — fetch the project's `[require]` git dependencies into an offline `vendor/`.\n\n\
                     Clones each dependency at its pinned tag/rev, copies its source into\n\
                     `vendor/<vendor>/<package>/`, and writes `phorge.lock` (resolved SHA + content\n\
                     hash). This is the only command that touches the network; commit `vendor/` +\n\
                     `phorge.lock` so `run`/`check`/`transpile` resolve fully offline.\n\n\
                     usage:\n  phg vendor [project-dir | phorge.toml]   (defaults to .)\n\n\
                     examples:\n  \
                     phg vendor\n  \
                     phg vendor path/to/project\n"
        }
        "serve" => {
            "serve — serve the program over HTTP/1.1 on a single thread.\n\n\
                    The program must define `respond(bytes) -> bytes`: the runtime frames each\n\
                    incoming request, calls `respond` (where the program's own `parse_request` /\n\
                    router / `serialize_response` live — all pure Phorge), and writes the bytes back\n\
                    (`Connection: close`, one request per connection). A request fault degrades to a\n\
                    500; a malformed request is the program's concern (→ a 400 from `respond`).\n\n\
                    The server is SINGLE-THREADED (the Rc-shared heap is not Send), so it handles one\n\
                    connection at a time. Bind 127.0.0.1 (the default) on untrusted networks, and use\n\
                    --timeout so a slow/idle client cannot wedge it (slowloris). A per-connection\n\
                    read/write error never ends the server — it is logged and the next connection is\n\
                    served.\n\n\
                    usage:\n  phg serve <file> [--addr 127.0.0.1:8080] [--timeout SECONDS]\n\n\
                    options:\n  \
                    --addr ADDR        host:port to bind (default 127.0.0.1:8080)\n  \
                    --timeout SECONDS  per-connection read/write timeout; 0 = none (default 30)\n  \
                    --dev              rich HTML error page on an uncaught fault (DEV ONLY; prod = bare 500)\n\n\
                    examples:\n  \
                    phg serve examples/web/server.phg\n  \
                    phg serve app.phg --addr 0.0.0.0:3000 --timeout 15\n"
        }
        _ => return help_text(),
    };
    format!("{}\n{body}", version_line())
}

/// `vendor [project-dir | phorge.toml]`: fetch the project's `[require]` git dependencies into an
/// offline `vendor/` tree and (re)write `phorge.lock`. `arg` is a directory or a manifest path
/// (default `.`); the project root is found by walking up to a `phorge.toml`. The only network-
/// touching command — see [`crate::vendor`].
pub fn cmd_vendor(arg: &str) -> Result<String, String> {
    let start = std::path::Path::new(arg);
    match crate::manifest::Project::detect(start)? {
        Some(project) => crate::vendor::vendor(&project),
        None => Err(format!(
            "no phorge.toml found at or above `{arg}` — `phg vendor` requires a project \
             (add a phorge.toml with a [require] section)"
        )),
    }
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
    resolve_source_and_args(rest).map(|(spec, _)| spec)
}

/// Like [`resolve_source`], but also returns the program's arguments (`Core.Process.args()`), taken
/// from a `--` terminator (Q5 of the Process-I/O design). Grammar:
/// `<file> [-- arg…]` | `- [-- arg…]` | `-e <code> [-- arg…]` | `-- <file> [-- arg…]`.
/// The **leading** `--` is the existing literal-path escape (`-- <file>`, for a path beginning with
/// `-`); a *non-leading* `--` separates phg's source-spec from the program's argv. So
/// `phg run app.phg -- a b` → `File(app.phg)` + `["a","b"]`, while `phg run -- -weird.phg -- a` →
/// `File(-weird.phg)` + `["a"]`. Returns `None` on a usage error (the caller prints usage, exits 2).
pub fn resolve_source_and_args(rest: &[String]) -> Option<(SourceSpec, Vec<String>)> {
    // Leading `--`: literal-path escape. `-- <file>` (no argv) | `-- <file> -- <argv…>`.
    if rest.first().map(String::as_str) == Some("--") {
        return match &rest[1..] {
            [path] => Some((SourceSpec::File(path.clone()), Vec::new())),
            [path, sep, args @ ..] if sep == "--" => {
                Some((SourceSpec::File(path.clone()), args.to_vec()))
            }
            _ => None,
        };
    }
    // Otherwise split the source-spec (before the first `--`) from the program argv (after it).
    let (head, args) = match rest.iter().position(|a| a == "--") {
        Some(i) => (&rest[..i], rest[i + 1..].to_vec()),
        None => (rest, Vec::new()),
    };
    let spec = match head {
        [flag, code] if flag == "-e" || flag == "--eval" => SourceSpec::Inline(code.clone()),
        [one] if one == "-" => SourceSpec::Stdin,
        [one] if !one.starts_with('-') => SourceSpec::File(one.clone()),
        _ => return None,
    };
    Some((spec, args))
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

/// Public lex + parse of a single source string into an **unchecked** `Program` (no type-check, no
/// alias/generic expansion). Exposes the private [`lex_parse`] for callers that want to run the
/// type-checker themselves and surface its diagnostics without aborting — e.g. the WASM playground,
/// which feeds the parsed program to [`check_json_program`] to render errors *and* warnings rather
/// than the fatal first-error string [`parse_checked`] produces. A syntax error still returns `Err`.
pub fn parse_program(src: &str) -> Result<Program, String> {
    lex_parse(src)
}

/// Type-check + de-alias an already-parsed program (the gate, minus lex/parse). De-aliases so every
/// backend sees alias-free types (aliases are front-end sugar; the checker validated them, including
/// cycles + built-in shadowing). Non-fatal warnings (the lint channel, M3 S2.5) render to stderr and
/// never gate the build. `diag_src` is the source used to render error carets — the single file for a
/// loose program, or `""` for a merged multi-file unit (where no single source aligns, so diagnostics
/// print message + position without a source line).
/// The canonical `Core.Json` value model, injected (below) when a program imports `Core.Json`. A
/// recursive enum over the JSON shapes; `Int`/`Float` are distinct (PHP-faithful, design-locked).
const JSON_PRELUDE: &str = "enum Json { Null(), Bool(bool value), Int(int value), \
     Float(float value), Str(string value), Arr(List<Json> items), Obj(Map<string, Json> entries) }";

/// Inject the `Json` enum at the head of a program that imports `Core.Json`, so the `Core.Json.*`
/// natives' `Json`-typed signatures resolve and user code can construct/`match` the variants — the
/// enum then flows through every backend as an ordinary enum (`docs/specs/2026-06-26-core-json-design.md`).
/// Runs before `check_resolutions` (below), the single chokepoint covering run/runvm/transpile + the
/// loader. A no-op (borrowed) unless `Core.Json` is imported and no `Json` enum is already declared.
fn inject_json_prelude(prog: &Program) -> std::borrow::Cow<'_, Program> {
    use crate::ast::Item;
    let imports_json = prog.items.iter().any(|it| {
        matches!(it, Item::Import { path, type_only: false, .. }
            if path.len() == 2 && path[0] == "Core" && path[1] == "Json")
    });
    let already_declared = prog
        .items
        .iter()
        .any(|it| matches!(it, Item::Enum(e) if e.name == "Json"));
    if !imports_json || already_declared {
        return std::borrow::Cow::Borrowed(prog);
    }
    match lex_parse(JSON_PRELUDE)
        .ok()
        .and_then(|p| p.items.into_iter().find(|i| matches!(i, Item::Enum(_))))
    {
        Some(enum_item) => {
            let mut items = Vec::with_capacity(prog.items.len() + 1);
            items.push(enum_item);
            items.extend(prog.items.iter().cloned());
            std::borrow::Cow::Owned(Program {
                package: prog.package.clone(),
                items,
                span: prog.span,
            })
        }
        None => std::borrow::Cow::Borrowed(prog), // unreachable: JSON_PRELUDE is valid
    }
}

/// The canonical `RoundingMode` enum, injected (below) when a program imports `Core.Decimal`
/// (M-NUM S2). Zero-payload variants — constructed `new HalfUp()` and matched `HalfUp()`, the
/// project's zero-payload variant convention — read by `Decimal.div`/`Decimal.round` via the
/// variant name. The seven modes mirror `value::RoundMode`. (Same [[core-json-and-injected-types]]
/// injected-type pattern as `Json`.)
const ROUNDING_MODE_PRELUDE: &str =
    "enum RoundingMode { HalfUp(), HalfDown(), HalfEven(), Up(), Down(), Ceiling(), Floor() }";

/// Inject the `RoundingMode` enum at the head of a program that imports `Core.Decimal`, so the
/// `Decimal.div`/`Decimal.round` natives' `RoundingMode`-typed signatures resolve and user code can
/// construct the variants (`new HalfUp()`) — the enum then flows through every backend as an ordinary
/// enum. Mirrors [`inject_json_prelude`]: a no-op (borrowed) unless `Core.Decimal` is imported and no
/// `RoundingMode` enum is already declared.
fn inject_rounding_mode_prelude(prog: &Program) -> std::borrow::Cow<'_, Program> {
    use crate::ast::Item;
    let imports_decimal = prog.items.iter().any(|it| {
        matches!(it, Item::Import { path, type_only: false, .. }
            if path.len() == 2 && path[0] == "Core" && path[1] == "Decimal")
    });
    let already_declared = prog
        .items
        .iter()
        .any(|it| matches!(it, Item::Enum(e) if e.name == "RoundingMode"));
    if !imports_decimal || already_declared {
        return std::borrow::Cow::Borrowed(prog);
    }
    match lex_parse(ROUNDING_MODE_PRELUDE)
        .ok()
        .and_then(|p| p.items.into_iter().find(|i| matches!(i, Item::Enum(_))))
    {
        Some(enum_item) => {
            let mut items = Vec::with_capacity(prog.items.len() + 1);
            items.push(enum_item);
            items.extend(prog.items.iter().cloned());
            std::borrow::Cow::Owned(Program {
                package: prog.package.clone(),
                items,
                span: prog.span,
            })
        }
        None => std::borrow::Cow::Borrowed(prog), // unreachable: ROUNDING_MODE_PRELUDE is valid
    }
}

/// The canonical `Core.Http` types, injected (below) when a program imports `Core.Http` (M6 W1 →
/// stdlib). The portable handler model — `handle(Request) -> Response` — at the value level: `Request`
/// and `Response` are immutable values; `Request.parse(bytes) -> Request?` and `resp.serialize()`
/// round-trip the HTTP/1.1 wire form. The bodies reuse `Core.Bytes`/`Core.Text` (so the prelude also
/// imports them), so this is the same proven logic as `examples/web/handler.phg`, promoted to the
/// stdlib behind the static-method API (slice B0). Flows through every backend as ordinary classes.
const HTTP_PRELUDE: &str = r#"
import Core.Bytes;
import Core.Text;
class Request {
  constructor(public string method, public string path, public bytes body, private List<string> headerLines) {}
  function header(string name): string? {
    for (string line in this.headerLines) {
      if (Text.contains(line, ":")) {
        List<string> kv = Text.splitOnce(line, ":");
        string key = Text.trim(kv[0]);
        if (key == name) { return Text.trim(kv[1]); }
      }
    }
    return null;
  }
  static function parse(bytes raw): Request? {
    int sep = Bytes.find(raw, b"\x0d\x0a\x0d\x0a") ?? -1;
    if (sep < 0) { return null; }
    bytes headBytes = Bytes.slice(raw, 0, sep);
    bytes body = Bytes.slice(raw, sep + 4, Bytes.length(raw));
    string head = Bytes.toString(headBytes) ?? "";
    string nl = Bytes.toString(b"\x0d\x0a") ?? "";
    List<string> lines = Text.split(head, nl);
    string requestLine = lines[0];
    List<string> rl = Text.split(requestLine, " ");
    string method = rl[0];
    string path = rl[1];
    return new Request(method, path, body, lines);
  }
}
class Response {
  constructor(public int status, public bytes body, public List<string> headerLines) {}
  static function text(int status, string body): Response {
    return new Response(status, Bytes.fromString(body), ["Content-Type: text/plain"]);
  }
  static function reason(int s): string {
    return if (s == 200) { "OK" }
      else { if (s == 400) { "Bad Request" }
      else { if (s == 404) { "Not Found" }
      else { "Internal Server Error" } } };
  }
  function serialize(): bytes {
    string nl = Bytes.toString(b"\x0d\x0a") ?? "";
    string reason = Response.reason(this.status);
    int st = this.status;
    string statusLine = "HTTP/1.1 {st} {reason}";
    int bodyLen = Bytes.length(this.body);
    string userHeaders = Text.join(this.headerLines, nl);
    string head = "{statusLine}{nl}Content-Length: {bodyLen}{nl}{userHeaders}{nl}{nl}";
    return Bytes.concat(Bytes.fromString(head), this.body);
  }
}
"#;

/// The `phg serve` bridge: the runtime's `respond(bytes) -> bytes` entry, synthesized to wrap a
/// user-defined `handle(Request) -> Response` (closes Batch-1 C). Injected only when `Core.Http` is
/// imported, a `handle` exists, and the user hasn't written their own `respond`. A malformed request
/// (parse returns null) becomes a 400 — HTTP policy lives here in Phorge, not in the Rust runtime.
const HTTP_RESPOND_BRIDGE: &str = r#"
function respond(bytes raw): bytes {
  if (var req = Request.parse(raw)) {
    return handle(req).serialize();
  }
  return Response.text(400, "Bad Request").serialize();
}
"#;

/// Inject the `Core.Http` types (and, when applicable, the `respond` serve bridge) into a program that
/// imports `Core.Http`. Mirrors [`inject_json_prelude`]: a no-op (borrowed) unless `Core.Http` is
/// imported. Each piece is injected only if absent — a user may declare their own `Request`/`Response`
/// or `respond` and it wins. The `Core.Bytes`/`Core.Text` imports the bodies need are injected too
/// (skipped if the user already imports them).
fn inject_http_prelude(prog: &Program) -> std::borrow::Cow<'_, Program> {
    use crate::ast::Item;
    let imports = |m: &str| {
        prog.items.iter().any(
            |it| matches!(it, Item::Import { path, type_only: false, .. } if path.join(".") == m),
        )
    };
    if !imports("Core.Http") {
        return std::borrow::Cow::Borrowed(prog);
    }
    let has_class = |n: &str| {
        prog.items
            .iter()
            .any(|it| matches!(it, Item::Class(c) if c.name == n))
    };
    let has_fn = |n: &str| {
        prog.items
            .iter()
            .any(|it| matches!(it, Item::Function(f) if f.name == n))
    };
    let Some(parsed) = lex_parse(HTTP_PRELUDE).ok() else {
        return std::borrow::Cow::Borrowed(prog); // unreachable: HTTP_PRELUDE is valid
    };
    let mut prepend: Vec<Item> = Vec::new();
    for it in parsed.items {
        match &it {
            Item::Import { path, .. } if !imports(&path.join(".")) => prepend.push(it),
            Item::Class(c) if c.name == "Request" && !has_class("Request") => prepend.push(it),
            Item::Class(c) if c.name == "Response" && !has_class("Response") => prepend.push(it),
            _ => {}
        }
    }
    // The serve bridge: wrap the user's `handle` when present and no `respond` is defined.
    if has_fn("handle") && !has_fn("respond") {
        if let Ok(bridge) = lex_parse(HTTP_RESPOND_BRIDGE) {
            prepend.extend(
                bridge
                    .items
                    .into_iter()
                    .filter(|it| matches!(it, Item::Function(f) if f.name == "respond")),
            );
        }
    }
    if prepend.is_empty() {
        return std::borrow::Cow::Borrowed(prog);
    }
    let mut items = Vec::with_capacity(prog.items.len() + prepend.len());
    items.extend(prepend);
    items.extend(prog.items.iter().cloned());
    std::borrow::Cow::Owned(Program {
        package: prog.package.clone(),
        items,
        span: prog.span,
    })
}

pub fn check_and_expand(prog: &Program, diag_src: &str) -> Result<Program, String> {
    let json_injected = inject_json_prelude(prog);
    let rm_injected = inject_rounding_mode_prelude(json_injected.as_ref());
    let injected = inject_http_prelude(rm_injected.as_ref());
    let prog = injected.as_ref();
    match crate::checker::check_resolutions(prog) {
        Ok((warnings, html, ufcs)) => {
            for w in &warnings {
                eprintln!("warning: {}", w.render(diag_src));
            }
            // De-alias types, erase `html"…"` literals into their `Html.concat([…])` kernel calls
            // (built by the checker, keyed by span), then erase generic type parameters — all three
            // are front-end sugar removed before any backend runs (M-RT S7 adds the last).
            // Feature C: `unwrap_new` strips the `Expr::New` construction wrapper after the type sugar
            // is gone, so every backend sees the plain construction `Call`. Slice 6: `rewrite_ufcs`
            // runs last, rewriting each resolved `x.f(a)` member call into the ordinary free/native
            // call `f(x, a)` the checker chose — by then the receiver/args are fully de-sugared.
            // Batch D: inject `= null` defaults for optional instance fields (after aliases are
            // expanded, so an aliased optional is already `Type::Optional`) — a front-end desugar so
            // every backend initializes them identically.
            Ok(crate::checker::rewrite_ufcs(
                crate::checker::unwrap_new(crate::checker::erase_generics(
                    crate::checker::resolve_html(
                        crate::checker::inject_optional_field_defaults(
                            crate::checker::expand_aliases(prog),
                        ),
                        &html,
                    ),
                )),
                &ufcs,
            ))
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

/// Public lex + parse + check of a single source string into a checked, alias-expanded `Program`.
/// Exposes the private [`parse_checked`] pipeline for callers that need a ready-to-run program from
/// inline source — e.g. `tests/serve.rs`, which builds a serve program then drives it through
/// [`crate::serve::serve`] over an in-memory transport.
pub fn parse_checked_program(src: &str) -> Result<Program, String> {
    parse_checked(src)
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

/// Like [`cmd_run`], but also returns `main`'s exit code (Batch-1 B). The string source path
/// (`-e`/stdin and standalone built binaries); the project-loader path is [`run_program_exit`].
pub fn cmd_run_exit(src: &str) -> Result<(String, i64), String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        interpret_main(&prog).map_err(|e| e.to_string())
    })
}

/// Like [`cmd_runvm`], but also returns `main`'s exit code (Batch-1 B).
pub fn cmd_runvm_exit(src: &str) -> Result<(String, i64), String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        let program = compile(&prog).map_err(|e| e.to_string())?;
        Vm::new(&program).run_main().map_err(|e| e.to_string())
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

/// `run` on a loaded [`Unit`] (interpreter backend). A runtime fault is rendered **with its stack
/// trace** (error-handling slice 1): frames are attributed to files via the unit's `fn_files`, and the
/// caret is drawn against the innermost frame's source (project mode) or the single `diag_src` (loose).
pub fn run_program(unit: &crate::loader::Unit) -> Result<String, String> {
    on_deep_stack(|| {
        let checked = check_and_expand(&unit.program, &unit.diag_src)?;
        interpret(&checked).map_err(|mut e| {
            let src = unit.attribute_frames(&mut e);
            e.render(&src)
        })
    })
}

/// `runvm` on a loaded [`Unit`] (bytecode + VM backend). Same trace rendering as [`run_program`].
pub fn runvm_program(unit: &crate::loader::Unit) -> Result<String, String> {
    on_deep_stack(|| {
        let checked = check_and_expand(&unit.program, &unit.diag_src)?;
        let program = compile(&checked).map_err(|e| e.to_string())?;
        Vm::new(&program).run().map_err(|mut e| {
            let src = unit.attribute_frames(&mut e);
            e.render(&src)
        })
    })
}

/// Like [`run_program`], but also returns `main`'s exit code (Batch-1 B). `phg run <file>` uses this
/// to set the process exit status; the stdout-only [`run_program`] stays for the differential.
pub fn run_program_exit(unit: &crate::loader::Unit) -> Result<(String, i64), String> {
    on_deep_stack(|| {
        let checked = check_and_expand(&unit.program, &unit.diag_src)?;
        interpret_main(&checked).map_err(|mut e| {
            let src = unit.attribute_frames(&mut e);
            e.render(&src)
        })
    })
}

/// Like [`runvm_program`], but also returns `main`'s exit code (Batch-1 B).
pub fn runvm_program_exit(unit: &crate::loader::Unit) -> Result<(String, i64), String> {
    on_deep_stack(|| {
        let checked = check_and_expand(&unit.program, &unit.diag_src)?;
        let program = compile(&checked).map_err(|e| e.to_string())?;
        Vm::new(&program).run_main().map_err(|mut e| {
            let src = unit.attribute_frames(&mut e);
            e.render(&src)
        })
    })
}

/// `check` on an already-loaded program.
pub fn check_program(prog: &Program, diag_src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        check_and_expand(prog, diag_src)?;
        Ok("OK (type-checks clean)\n".to_string())
    })
}

/// `check --json` on an already-loaded program: machine-readable diagnostics for editor / LSP
/// integration (the seam `diagnostic.rs` calls out). Returns the JSON array (errors then warnings; see
/// [`crate::diagnostic::diagnostics_json`]) and whether any *error* was present, so the caller prints
/// the array to **stdout** and exits 0 (clean / warnings only) or 1 (errors) — `check`'s exit
/// semantics, but the array is always the output and nothing goes to stderr. Positions ride on each
/// diagnostic, so no `diag_src` is needed.
pub fn check_json_program(prog: &Program) -> (String, bool) {
    on_deep_stack(|| match crate::checker::check_resolutions(prog) {
        Ok((warnings, _html, _ufcs)) => {
            (crate::diagnostic::diagnostics_json(&[], &warnings), false)
        }
        Err(errs) => (crate::diagnostic::diagnostics_json(&errs, &[]), true),
    })
}

/// `transpile` on an already-loaded program (emit PHP). Multi-namespace emission for a multi-package
/// project is S2c; S2b emits the existing flat form (correct for `package Main` / single-package).
pub fn transpile_program(prog: &Program, diag_src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let checked = check_and_expand(prog, diag_src)?;
        crate::transpile::emit(&checked)
    })
}

/// `serve` on an already-loaded program (M6 W4): type-check, then run the blocking HTTP serve loop
/// ([`crate::serve::serve_tcp`]) until the process is killed. Runs on the 256 MB deep-stack worker so
/// the interpreter's `MAX_CALL_DEPTH` guard has the same headroom `run`/`runvm` rely on (the
/// per-request `call_named` walks the native stack). Returns only on a bind/socket error.
pub fn serve_program(
    prog: &Program,
    diag_src: &str,
    addr: &str,
    timeout: Option<std::time::Duration>,
    dev: bool,
) -> Result<String, String> {
    on_deep_stack(|| {
        let checked = check_and_expand(prog, diag_src)?;
        crate::serve::serve_tcp(&checked, addr, timeout, dev).map_err(|e| format!("serve: {e}"))?;
        Ok(String::new())
    })
}

/// Build a standalone executable for the host from `src`. `input_path` names the source (used to
/// derive the default output name); `out_path` overrides it. Validates the program first (never emits
/// a broken binary), then delegates to `bundle::cross::build_host`, which reuses this phg binary as
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

/// `lift`: read PHP source, emit a Phorge **draft** (the inverse of `transpile`). Best-effort and
/// review-required — the output is prefixed with a `// lifted (verify)` banner so the contract is
/// visible in the file itself. Anything outside the Tier-1 lift subset is a clear `lift …` error
/// (never a silent guess). No `on_deep_stack`: the lift parser has its own depth guard.
pub fn cmd_lift(src: &str) -> Result<String, String> {
    let phorge = crate::lift::lifter::lift_source(src)?;
    Ok(format!(
        "// lifted (verify) — a best-effort PHP->Phorge draft; review before trusting it.\n{phorge}"
    ))
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
        "phg disasm — {} function(s), main = #{}\n",
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

#[cfg(test)]
mod tests;
