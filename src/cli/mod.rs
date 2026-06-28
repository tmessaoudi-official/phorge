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
         lift       PHP -> a Phorj draft (review required; inverse of transpile)\n  \
         disasm     print the compiled bytecode\n  \
         bench      benchmark run vs runvm (time + memory)\n  \
         build      compile to a standalone executable (-o <out>)\n  \
         vendor     fetch [require] git deps into an offline vendor/ (writes phorj.lock)\n  \
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
            "lift — read PHP, emit a Phorj **draft** (the inverse of transpile). Best-effort and\n       \
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
                   project root is the nearest ancestor holding a `phorj.toml`, else the current\n\
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
            "fmt — format Phorj source to canonical form (comment-preserving, meaning-preserving).\n\n\
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
                     `vendor/<vendor>/<package>/`, and writes `phorj.lock` (resolved SHA + content\n\
                     hash). This is the only command that touches the network; commit `vendor/` +\n\
                     `phorj.lock` so `run`/`check`/`transpile` resolve fully offline.\n\n\
                     usage:\n  phg vendor [project-dir | phorj.toml]   (defaults to .)\n\n\
                     examples:\n  \
                     phg vendor\n  \
                     phg vendor path/to/project\n"
        }
        "serve" => {
            "serve — serve the program over HTTP/1.1.\n\n\
                    The program must define `respond(bytes) -> bytes`: the runtime frames each\n\
                    incoming request, calls `respond` (where the program's own `parse_request` /\n\
                    router / `serialize_response` live — all pure Phorj), and writes the bytes back\n\
                    (`Connection: close`, one request per connection). A request fault degrades to a\n\
                    500; a malformed request is the program's concern (→ a 400 from `respond`).\n\n\
                    Concurrency (--workers, M6 W3): each request is handled on its own worker thread\n\
                    with its own value heap (the Rc heap is never shared — values don't cross threads),\n\
                    so the server scales across CPU cores. Default = number of cores; --workers 1 is the\n\
                    single-threaded server. Bind 127.0.0.1 (the default) on untrusted networks, and use\n\
                    --timeout so a slow/idle client cannot wedge a worker (slowloris). A per-connection\n\
                    read/write error never ends the server — it is logged and the next connection is\n\
                    served.\n\n\
                    usage:\n  phg serve <file> [--addr 127.0.0.1:8080] [--timeout SECONDS] [--workers N]\n\n\
                    options:\n  \
                    --addr ADDR        host:port to bind (default 127.0.0.1:8080)\n  \
                    --timeout SECONDS  per-connection read/write timeout; 0 = none (default 30)\n  \
                    --workers N        request concurrency; 1 = single-threaded (default = CPU cores)\n  \
                    --dev              rich HTML error page on an uncaught fault (DEV ONLY; prod = bare 500)\n\n\
                    examples:\n  \
                    phg serve examples/web/server.phg\n  \
                    phg serve app.phg --addr 0.0.0.0:3000 --timeout 15 --workers 8\n"
        }
        _ => return help_text(),
    };
    format!("{}\n{body}", version_line())
}

/// `vendor [project-dir | phorj.toml]`: fetch the project's `[require]` git dependencies into an
/// offline `vendor/` tree and (re)write `phorj.lock`. `arg` is a directory or a manifest path
/// (default `.`); the project root is found by walking up to a `phorj.toml`. The only network-
/// touching command — see [`crate::vendor`].
pub fn cmd_vendor(arg: &str) -> Result<String, String> {
    let start = std::path::Path::new(arg);
    match crate::manifest::Project::detect(start)? {
        Some(project) => crate::vendor::vendor(&project),
        None => Err(format!(
            "no phorj.toml found at or above `{arg}` — `phg vendor` requires a project \
             (add a phorj.toml with a [require] section)"
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
import Core.List;
import Core.Regex;
class Request {
  constructor(public string method, public string path, public bytes body, private List<string> headerLines, private List<string> attrs) {}
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
  function param(string name): string? {
    mutable int i = 0;
    int n = List.length(this.attrs);
    while (i + 1 < n) {
      if (this.attrs[i] == name) { return this.attrs[i + 1]; }
      i += 2;
    }
    return null;
  }
  function withParams(List<string> p): Request {
    return new Request(this.method, this.path, this.body, this.headerLines, p);
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
    return new Request(method, path, body, lines, []);
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
class Route {
  constructor(public string method, public string pattern, public (Request) -> Response handler) {}
}
class Router {
  // `table` = the registered routes; `mws` = middleware applied (outermost-first) to every matched
  // handler. Middleware is `(Request, next) -> Response`: it may call `next(req)` to continue the
  // chain, or short-circuit (e.g. return 401 without calling `next`).
  constructor(private List<Route> table, private List<(Request, (Request) -> Response) -> Response> mws) {}
  function route(string method, string pattern, (Request) -> Response handler): Router {
    return new Router(List.concat(this.table, [new Route(method, pattern, handler)]), this.mws);
  }
  // Append a middleware (applies to every route this router handles). Chainable, immutable.
  function use((Request, (Request) -> Response) -> Response mw): Router {
    return new Router(this.table, List.concat(this.mws, [mw]));
  }
  // Mount a sub-router under `prefix`: run `build` on a fresh empty router, then merge each sub-route
  // with `prefix` prepended to its pattern and the sub-router's own middleware composed around its
  // handler (so group-scoped middleware applies). The parent's `use` middleware still applies on top
  // in `handle`.
  function group(string prefix, (Router) -> Router build): Router {
    var builder = build;
    Router sub = builder(new Router([], []));
    mutable List<Route> merged = this.table;
    for (Route r in sub.table) {
      var h = r.handler;
      var wrapped = Router.compose(sub.mws, h);
      merged = List.concat(merged, [new Route(r.method, prefix + r.pattern, wrapped)]);
    }
    return new Router(merged, this.mws);
  }
  // Fold a middleware list around a handler: first-registered runs OUTERMOST. Each step builds a
  // `fn(req) => mw(req, prev)` closure capturing the middleware and the previously-wrapped handler.
  static function compose(List<(Request, (Request) -> Response) -> Response> mws, (Request) -> Response handler): (Request) -> Response {
    mutable var h = handler;
    mutable int i = List.length(mws) - 1;
    while (i >= 0) {
      var mw = mws[i];
      var prev = h;
      h = fn(Request req) -> Response { return mw(req, prev); };
      i -= 1;
    }
    return h;
  }
  static function idStrs(List<string> xs): List<string> { return xs; }
  // A pattern segment is a parameter iff it is `{...}`. The inner text is `name` (bare) or
  // `name:regex` (constrained); the regex must match the WHOLE path segment.
  static function isParam(string seg): bool {
    return Text.startsWith(seg, "\{") && Text.endsWith(seg, "\}");
  }
  static function paramInner(string seg): string {
    // Drop only the OUTER braces (substring 1..len-1) — a constraint regex may itself contain braces
    // (`\d{4}`), so stripping every `{`/`}` would corrupt it. `substring(s, 1, -1)` = bytes[1..len-1]
    // on both backends and PHP `substr($s, 1, -1)`.
    return Text.substring(seg, 1, -1);
  }
  static function paramName(string seg): string {
    string inner = Router.paramInner(seg);
    if (Text.contains(inner, ":")) { List<string> kv = Text.splitOnce(inner, ":"); return kv[0]; }
    return inner;
  }
  // A constrained segment matches its path component iff the (whole-segment-anchored) regex matches.
  static function constraintOk(string seg, string component): bool {
    string inner = Router.paramInner(seg);
    if (Text.contains(inner, ":")) {
      List<string> kv = Text.splitOnce(inner, ":");
      var re = Regex.compile("^(?:" + kv[1] + ")$");
      return Regex.matches(re, component);
    }
    return true; // a bare `{name}` matches any component
  }
  // Specificity score (higher = more specific), or -1 for no match. A literal segment scores 2, a
  // matching CONSTRAINED param scores 1, a bare param scores 0 — so literal > constrained > param.
  // A constrained param whose component fails its regex makes the whole route not match.
  static function segScore(string pattern, string path): int {
    List<string> ps = Text.split(pattern, "/");
    List<string> xs = Text.split(path, "/");
    if (List.length(ps) != List.length(xs)) { return -1; }
    mutable int score = 0;
    mutable int i = 0;
    int n = List.length(ps);
    while (i < n) {
      string p = ps[i];
      if (Router.isParam(p)) {
        if (!Router.constraintOk(p, xs[i])) { return -1; }
        if (Text.contains(Router.paramInner(p), ":")) { score += 1; }
      } else {
        if (p != xs[i]) { return -1; }
        score += 2;
      }
      i += 1;
    }
    return score;
  }
  static function extractParams(string pattern, string path): List<string> {
    List<string> ps = Text.split(pattern, "/");
    List<string> xs = Text.split(path, "/");
    mutable List<string> out = Router.idStrs([]);
    mutable int i = 0;
    int n = List.length(ps);
    while (i < n) {
      string p = ps[i];
      if (Router.isParam(p)) {
        out = List.concat(out, [Router.paramName(p), xs[i]]);
      }
      i += 1;
    }
    return out;
  }
  function handle(Request req): Response {
    mutable int best = -1;
    mutable int bestScore = -1;
    mutable int idx = 0;
    for (Route r in this.table) {
      if (r.method == req.method) {
        int sc = Router.segScore(r.pattern, req.path);
        if (sc > bestScore) { best = idx; bestScore = sc; }
      }
      idx += 1;
    }
    if (best < 0) { return Response.text(404, "Not Found: {req.method} {req.path}"); }
    Route chosen = this.table[best];
    List<string> params = Router.extractParams(chosen.pattern, req.path);
    var composed = Router.compose(this.mws, chosen.handler);
    return composed(req.withParams(params));
  }
}
"#;

/// The `phg serve` bridge: the runtime's `respond(bytes) -> bytes` entry, synthesized to wrap a
/// user-defined `handle(Request) -> Response` (closes Batch-1 C). Injected only when `Core.Http` is
/// imported, a `handle` exists, and the user hasn't written their own `respond`. A malformed request
/// (parse returns null) becomes a 400 — HTTP policy lives here in Phorj, not in the Rust runtime.
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
            Item::Class(c) if c.name == "Route" && !has_class("Route") => prepend.push(it),
            Item::Class(c) if c.name == "Router" && !has_class("Router") => prepend.push(it),
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

/// The opaque compiled-`Regex` value model, injected when a program imports `Core.Regex` (Fork A,
/// `docs/specs/2026-06-28-core-regex-design.md`). A `Regex` value is built only by `Regex.compile`
/// (which validates via the `regex` crate); the `pattern` field is the **bare** pattern. It is public
/// so the transpiled `__phorj_regex_*` global helpers can read `$re->pattern` to build the
/// `/u`-delimited PHP `preg_*` form. Mirrors [`inject_json_prelude`]: a no-op unless `Core.Regex` is
/// imported and no `Regex` class is already declared.
const REGEX_PRELUDE: &str = "class Regex { constructor(public string pattern) {} }";

fn inject_regex_prelude(prog: &Program) -> std::borrow::Cow<'_, Program> {
    use crate::ast::Item;
    let imports_regex = prog.items.iter().any(|it| {
        matches!(it, Item::Import { path, type_only: false, .. }
            if path.len() == 2 && path[0] == "Core" && path[1] == "Regex")
    });
    let already_declared = prog
        .items
        .iter()
        .any(|it| matches!(it, Item::Class(c) if c.name == "Regex"));
    if !imports_regex || already_declared {
        return std::borrow::Cow::Borrowed(prog);
    }
    match lex_parse(REGEX_PRELUDE)
        .ok()
        .and_then(|p| p.items.into_iter().find(|i| matches!(i, Item::Class(_))))
    {
        Some(class_item) => {
            let mut items = Vec::with_capacity(prog.items.len() + 1);
            items.push(class_item);
            items.extend(prog.items.iter().cloned());
            std::borrow::Cow::Owned(Program {
                package: prog.package.clone(),
                items,
                span: prog.span,
            })
        }
        None => std::borrow::Cow::Borrowed(prog), // unreachable: REGEX_PRELUDE is valid
    }
}

/// The `Secret<T>` opaque-wrapper type, injected when a program imports `Core.Secret` (Fork B,
/// `docs/specs/2026-06-28-secret-type-design.md`). A `Secret<T>` value is constructed `new Secret(x)`
/// and read only through `expose()` — the `value` field is private, and a `Secret` instance is not a
/// `string`, so printing/interpolating it is a clean type error (the primary, loud guarantee; no
/// runtime `***`). Reuses the generic-class machinery (`Box<T>`) wholesale — no new `Op`/`Value`/`Ty`.
/// Mirrors [`inject_regex_prelude`]: a no-op unless `Core.Secret` is imported and no `Secret` class is
/// already declared. The transpiler adds `final` + `#[\SensitiveParameter]` for this class by name.
const SECRET_PRELUDE: &str =
    "class Secret<T> { constructor(private T value) {} function expose(): T { return this.value; } }";

fn inject_secret_prelude(prog: &Program) -> std::borrow::Cow<'_, Program> {
    use crate::ast::Item;
    let imports_secret = prog.items.iter().any(|it| {
        matches!(it, Item::Import { path, type_only: false, .. }
            if path.len() == 2 && path[0] == "Core" && path[1] == "Secret")
    });
    let already_declared = prog
        .items
        .iter()
        .any(|it| matches!(it, Item::Class(c) if c.name == "Secret"));
    if !imports_secret || already_declared {
        return std::borrow::Cow::Borrowed(prog);
    }
    match lex_parse(SECRET_PRELUDE)
        .ok()
        .and_then(|p| p.items.into_iter().find(|i| matches!(i, Item::Class(_))))
    {
        Some(class_item) => {
            let mut items = Vec::with_capacity(prog.items.len() + 1);
            items.push(class_item);
            items.extend(prog.items.iter().cloned());
            std::borrow::Cow::Owned(Program {
                package: prog.package.clone(),
                items,
                span: prog.span,
            })
        }
        None => std::borrow::Cow::Borrowed(prog), // unreachable: SECRET_PRELUDE is valid
    }
}

/// The `Core.Time` value model (M-TIME, `docs/specs/2026-06-28-m-time-design.md`): the pure-Phorj
/// `Instant`, `Duration`, `Date`, and `DateTime` classes. Because the prelude is run through the same
/// backends and transpiler as user code, all calendar and formatting math is byte-identical by
/// construction; the only native is the clock seam (the `Core.Time` module in `src/native/time.rs`).
/// The model is UTC-only because timezones are non-deterministic and would break the byte-identity
/// spine. Calendar math uses Hinnant's truncating-division-safe civil/day conversions, which port
/// verbatim since Phorj int division truncates toward zero (PHP `intdiv`).
const TIME_PRELUDE: &str = r#"
class Duration {
  constructor(public int ms) {}
  static function millis(int n) -> Duration { return new Duration(n); }
  static function seconds(int n) -> Duration { return new Duration(n * 1000); }
  static function minutes(int n) -> Duration { return new Duration(n * 60000); }
  static function hours(int n) -> Duration { return new Duration(n * 3600000); }
  static function days(int n) -> Duration { return new Duration(n * 86400000); }
  function toMillis() -> int { return this.ms; }
  function toSeconds() -> int { return this.ms / 1000; }
  function toMinutes() -> int { return this.ms / 60000; }
  function toHours() -> int { return this.ms / 3600000; }
  function toDays() -> int { return this.ms / 86400000; }
  function plus(Duration o) -> Duration { return new Duration(this.ms + o.ms); }
  function minus(Duration o) -> Duration { return new Duration(this.ms - o.ms); }
  function negate() -> Duration { return new Duration(-this.ms); }
  function isZero() -> bool { return this.ms == 0; }
  function isNegative() -> bool { return this.ms < 0; }
}
class Date {
  constructor(public int epochDay) {}
  // Howard Hinnant's days-from-civil / civil-from-days (truncating-division safe; Phorj int `/` is
  // truncate-toward-zero = PHP intdiv). `daysFromCivil`/`civil`/`pad2` are low-level building blocks
  // reused by `DateTime`; the everyday API is `of`/`year`/`month`/`day`/`addDays`/`toString`.
  static function daysFromCivil(int y, int m, int d) -> int {
    int yy = y - (if (m <= 2) { 1 } else { 0 });
    int era = (if (yy >= 0) { yy } else { yy - 399 }) / 400;
    int yoe = yy - era * 400;
    int doy = (153 * (if (m > 2) { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    int doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    return era * 146097 + doe - 719468;
  }
  static function civil(int z) -> List<int> {
    int zz = z + 719468;
    int era = (if (zz >= 0) { zz } else { zz - 146096 }) / 146097;
    int doe = zz - era * 146097;
    int yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    int y = yoe + era * 400;
    int doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    int mp = (5 * doy + 2) / 153;
    int d = doy - (153 * mp + 2) / 5 + 1;
    int m = if (mp < 10) { mp + 3 } else { mp - 9 };
    return [y + (if (m <= 2) { 1 } else { 0 }), m, d];
  }
  static function pad2(int n) -> string { return if (n < 10) { "0{n}" } else { "{n}" }; }
  // Zero-pad a non-negative year to 4 digits (ISO `YYYY`); proleptic negatives are emitted verbatim.
  static function pad4(int n) -> string {
    return if (n < 0) { "{n}" } else { if (n < 10) { "000{n}" } else { if (n < 100) { "00{n}" } else { if (n < 1000) { "0{n}" } else { "{n}" } } } };
  }
  static function of(int y, int m, int d) -> Date { return new Date(Date.daysFromCivil(y, m, d)); }
  static function ofEpochDay(int d) -> Date { return new Date(d); }
  function year() -> int { return Date.civil(this.epochDay)[0]; }
  function month() -> int { return Date.civil(this.epochDay)[1]; }
  function day() -> int { return Date.civil(this.epochDay)[2]; }
  function addDays(int n) -> Date { return new Date(this.epochDay + n); }
  function minusDays(int n) -> Date { return new Date(this.epochDay - n); }
  function daysUntil(Date o) -> int { return o.epochDay - this.epochDay; }
  // 1=Mon … 7=Sun (ISO-8601). epochDay 0 = 1970-01-01 = Thursday.
  function dayOfWeek() -> int {
    int w = (this.epochDay + 3) % 7;
    return (if (w < 0) { w + 7 } else { w }) + 1;
  }
  function isLeapYear() -> bool {
    int y = this.year();
    return (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
  }
  function isBefore(Date o) -> bool { return this.epochDay < o.epochDay; }
  function isAfter(Date o) -> bool { return this.epochDay > o.epochDay; }
  function compareTo(Date o) -> int {
    return if (this.epochDay < o.epochDay) { -1 } else { if (this.epochDay > o.epochDay) { 1 } else { 0 } };
  }
  function toString() -> string {
    List<int> c = Date.civil(this.epochDay);
    return "{Date.pad4(c[0])}-{Date.pad2(c[1])}-{Date.pad2(c[2])}";
  }
}
class Instant {
  constructor(public int ms) {}
  static function ofEpochMillis(int m) -> Instant { return new Instant(m); }
  static function ofEpochSeconds(int s) -> Instant { return new Instant(s * 1000); }
  static function now() -> Instant { return new Instant(Time.nowMillis()); }
  function epochMillis() -> int { return this.ms; }
  function epochSeconds() -> int { return this.ms / 1000; }
  function plus(Duration d) -> Instant { return new Instant(this.ms + d.ms); }
  function minus(Duration d) -> Instant { return new Instant(this.ms - d.ms); }
  function durationSince(Instant o) -> Duration { return new Duration(this.ms - o.ms); }
  function isBefore(Instant o) -> bool { return this.ms < o.ms; }
  function isAfter(Instant o) -> bool { return this.ms > o.ms; }
  function compareTo(Instant o) -> int {
    return if (this.ms < o.ms) { -1 } else { if (this.ms > o.ms) { 1 } else { 0 } };
  }
  // Civil-date view (UTC, day-resolution): floor-divide millis by a day (floor, not truncate, so a
  // pre-1970 instant maps to the right civil day).
  function toDate() -> Date {
    int day = if (this.ms >= 0) { this.ms / 86400000 } else { (this.ms - 86399999) / 86400000 };
    return Date.ofEpochDay(day);
  }
  // ── civil (wall-time) view, UTC ──────────────────────────────────────────────────────────────
  // An `Instant` is also the human date-time: it exposes year/month/day/hour/minute/second/millis and
  // an ISO-8601 string. (No separate `DateTime` class — that name collides with PHP's built-in, and
  // `Instant` already IS the point in time; fields are derived on demand.) `ofCivil` builds an instant
  // from broken-down UTC fields.
  static function ofCivil(int y, int mo, int d, int h, int mi, int s) -> Instant {
    int day = Date.daysFromCivil(y, mo, d);
    return new Instant(day * 86400000 + h * 3600000 + mi * 60000 + s * 1000);
  }
  // Milliseconds within the current UTC day, always in [0, 86399999] (uses the floored epoch-day).
  function millisOfDay() -> int {
    int day = if (this.ms >= 0) { this.ms / 86400000 } else { (this.ms - 86399999) / 86400000 };
    return this.ms - day * 86400000;
  }
  function year() -> int { return this.toDate().year(); }
  function month() -> int { return this.toDate().month(); }
  function day() -> int { return this.toDate().day(); }
  function dayOfWeek() -> int { return this.toDate().dayOfWeek(); }
  function hour() -> int { return this.millisOfDay() / 3600000; }
  function minute() -> int { return (this.millisOfDay() / 60000) % 60; }
  function second() -> int { return (this.millisOfDay() / 1000) % 60; }
  function millis() -> int { return this.millisOfDay() % 1000; }
  // ISO-8601 UTC: `YYYY-MM-DDTHH:MM:SSZ` (always `Z`; second-resolution, sub-second dropped). For any
  // other layout, interpolate the accessors directly (Phorj has first-class string interpolation).
  function toIso() -> string {
    List<int> c = Date.civil(this.toDate().epochDay);
    string date = "{Date.pad4(c[0])}-{Date.pad2(c[1])}-{Date.pad2(c[2])}";
    string time = "{Date.pad2(this.hour())}:{Date.pad2(this.minute())}:{Date.pad2(this.second())}";
    return "{date}T{time}Z";
  }
}
"#;

fn inject_time_prelude(prog: &Program) -> std::borrow::Cow<'_, Program> {
    use crate::ast::Item;
    let imports_time = prog.items.iter().any(|it| {
        matches!(it, Item::Import { path, type_only: false, .. }
            if path.len() == 2 && path[0] == "Core" && path[1] == "Time")
    });
    if !imports_time {
        return std::borrow::Cow::Borrowed(prog);
    }
    let has_class = |n: &str| {
        prog.items
            .iter()
            .any(|it| matches!(it, Item::Class(c) if c.name == n))
    };
    let Ok(parsed) = lex_parse(TIME_PRELUDE) else {
        return std::borrow::Cow::Borrowed(prog); // unreachable: TIME_PRELUDE is valid
    };
    let prepend: Vec<Item> = parsed
        .items
        .into_iter()
        .filter(|it| matches!(it, Item::Class(c) if !has_class(&c.name)))
        .collect();
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
    let http_injected = inject_http_prelude(rm_injected.as_ref());
    let regex_injected = inject_regex_prelude(http_injected.as_ref());
    let secret_injected = inject_secret_prelude(regex_injected.as_ref());
    let injected = inject_time_prelude(secret_injected.as_ref());
    // M6 W2: lower `Http.autoRouter()` into explicit `Router` construction from the `#[Route]`-
    // annotated handlers — BEFORE the checker, so the generated registration type-checks like
    // hand-written code (a no-op unless `Core.Http` is imported). The `#[Route]` attrs survive for the
    // checker's validation pass, then are inert for the backends.
    let routed = crate::checker::desugar_auto_router(injected.into_owned());
    let prog = &routed;
    match crate::checker::check_resolutions(prog) {
        Ok((warnings, html, ufcs, overload_renames)) => {
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
            // Slice C1: rename each return-overload member's *definition* to its mangled name (by decl
            // span); the resolved selector *call sites* were already merged into `ufcs` above and are
            // rewritten to the same mangled names by `rewrite_ufcs`. A no-op when no function is
            // return-overloaded (so single-overload programs stay byte-identical).
            Ok(crate::checker::rename_overload_defs(
                crate::checker::rewrite_ufcs(
                    crate::checker::unwrap_new(crate::checker::erase_generics(
                        crate::checker::resolve_html(
                            crate::checker::inject_optional_field_defaults(
                                crate::checker::expand_aliases(prog),
                            ),
                            &html,
                        ),
                    )),
                    &ufcs,
                ),
                &overload_renames,
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
/// M8.5 interop: refuse to *execute* a program that uses foreign `declare` symbols. The Rust backends
/// (interpreter/VM) have no PHP runtime, so a foreign call cannot run — the program is PHP-target-only.
/// `check`/`transpile` work fully; only `run`/`runvm` hit this one clean pre-flight gate (no per-call
/// fault machinery in the execution paths). A single scan after type-checking, before any backend.
fn foreign_runtime_gate(prog: &Program) -> Result<(), String> {
    use crate::ast::Item;
    let has_foreign = prog.items.iter().any(|it| {
        matches!(it, Item::Function(f) if f.foreign) || matches!(it, Item::Class(c) if c.foreign)
    });
    if has_foreign {
        return Err(
            "error[E-FOREIGN-RUNTIME]: this program declares foreign PHP symbols (`declare`), \
             which require the PHP runtime to execute. The Rust backends (run/runvm) have no PHP \
             runtime — transpile it instead: `phg transpile <file> > out.php && php out.php`.\n"
                .to_string(),
        );
    }
    Ok(())
}

pub fn cmd_run(src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        foreign_runtime_gate(&prog)?;
        interpret(&prog).map_err(|e| e.to_string())
    })
}

/// `runvm`: lex -> parse -> check (gate) -> compile to bytecode -> VM -> captured stdout.
/// The bytecode backend; must produce byte-identical output to `cmd_run` (differential).
pub fn cmd_runvm(src: &str) -> Result<String, String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        foreign_runtime_gate(&prog)?;
        let program = compile(&prog).map_err(|e| e.to_string())?;
        Vm::new(&program).run().map_err(|e| e.to_string())
    })
}

/// Like [`cmd_run`], but also returns `main`'s exit code (Batch-1 B). The string source path
/// (`-e`/stdin and standalone built binaries); the project-loader path is [`run_program_exit`].
pub fn cmd_run_exit(src: &str) -> Result<(String, i64), String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        foreign_runtime_gate(&prog)?;
        interpret_main(&prog).map_err(|e| e.to_string())
    })
}

/// Like [`cmd_runvm`], but also returns `main`'s exit code (Batch-1 B).
pub fn cmd_runvm_exit(src: &str) -> Result<(String, i64), String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        foreign_runtime_gate(&prog)?;
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
        foreign_runtime_gate(&checked)?;
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
        foreign_runtime_gate(&checked)?;
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
        foreign_runtime_gate(&checked)?;
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
        foreign_runtime_gate(&checked)?;
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
        Ok((warnings, _html, _ufcs, _ovl)) => {
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
    workers: usize,
) -> Result<String, String> {
    on_deep_stack(|| {
        let checked = check_and_expand(prog, diag_src)?;
        crate::serve::serve_tcp(&checked, addr, timeout, dev, workers)
            .map_err(|e| format!("serve: {e}"))?;
        Ok(String::new())
    })
}

/// Build a standalone executable for the host from `src`. `input_path` names the source (used to
/// derive the default output name); `out_path` overrides it. Validates the program first (never emits
/// a broken binary), then delegates to `bundle::cross::build_host`, which reuses this phg binary as
/// the stub and embeds `src` as a `.phorj` section. Returns a one-line success message.
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

/// `lift`: read PHP source, emit a Phorj **draft** (the inverse of `transpile`). Best-effort and
/// review-required — the output is prefixed with a `// lifted (verify)` banner so the contract is
/// visible in the file itself. Anything outside the Tier-1 lift subset is a clear `lift …` error
/// (never a silent guess). No `on_deep_stack`: the lift parser has its own depth guard.
pub fn cmd_lift(src: &str) -> Result<String, String> {
    let phorj = crate::lift::lifter::lift_source(src)?;
    Ok(format!(
        "// lifted (verify) — a best-effort PHP->Phorj draft; review before trusting it.\n{phorj}"
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
