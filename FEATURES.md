# Features

A capability matrix for Phorge — what works **today** versus what is **planned**. For runnable proof
of the "today" column, see [`examples/`](examples/README.md); for the forward plan see
[ROADMAP.md](ROADMAP.md); for things that are deliberately rejected-but-clean, see
[KNOWN_ISSUES.md](KNOWN_ISSUES.md).

## Language

| Feature | Status | Notes |
|---|---|---|
| Static types: `int`, `float`, `bool`, `string` | ✅ | checked at compile time |
| Raw bytes: `bytes` + `b"…"` literals (`\xHH`) | ✅ | octet sequences distinct from UTF-8 `string`; `core.bytes` interop (`from_string`/`to_string`/`len`/`concat`/`slice`/`find`) |
| Typed HTML: `Html`/`Attr` + `core.html` kernel, builders & `html"…"` sugar | ✅ | distinct from `string` (XSS-safe by construction); kernel `text` (auto-escape) / `raw` (audited trust) / `render`; builders `el` / `void_el` / `attr` / `bool_attr` / `concat` + named per-tag helpers (`div`/`p`/`a`/`ul`/`li`/`br`/`img`/…, macro-baked); `html"<h1>{name}</h1>"` literal sugar — holes escape by type unless already `Html`, desugars to kernel calls (no new `Op`) |
| Empty list literal `[]` in call arguments | ✅ | takes its element type from the expected parameter (e.g. `el("p", [], […])`); other positions still need a non-empty literal |
| Generic lists: `List<T>` + list literals | ✅ | `[1, 2, 3]` |
| Immutable-by-default bindings | ✅ | no reassignment; fresh binding instead |
| Functions + recursion | ✅ | `function f(int n) -> int { … }`, `main()` entry point |
| Classes + fields + methods (`this`) | ✅ | |
| Constructor promotion | ✅ | `constructor(private int total) {}` |
| Enums with payloads | ✅ | `enum Shape { Circle(float r), Rect(float w, float h) }` |
| `match` (exhaustiveness-checked) | ✅ | over enum variants |
| String interpolation | ✅ | `"area = {area(s)}"` |
| `for … in` over lists | ✅ | `for (int s in [80, 30, 55]) { … }` |
| `if` / `else`, blocks, comparison, equality, `&&`/`||`, unary | ✅ | short-circuit logical ops |
| Checked arithmetic | ✅ | int overflow & div-by-zero → clean runtime error, never a panic |
| Local type inference: `var x = …;` | ✅ | inferred from the initializer; still fully static + immutable |
| Type aliases: `type Name = T;` | ✅ | compile-time only, erased in the PHP output |
| Indexing `xs[i]` | ✅ | bounds-checked; out-of-range → clean runtime fault, never a panic |
| Integer ranges `a..b` / `a..=b` | ✅ | materialize to `List<int>`; mainly `for (int i in 0..n)` |
| Expression `if` | ✅ | `var x = if (c) { 1 } else { 2 };` (value position; `else` required) |
| Lambdas / closures | ✅ | `fn(int x) => x * 2` (expression body) and `fn(int x) -> int { … }` (statement body, `-> T` required); capture enclosing locals by value |
| First-class function values | ✅ | a bare named function is a value (`twice(3, dbl)`); function types `(int) -> int`; transpile to PHP arrow fn / `function(){} use()` / first-class callable |
| `Map` / `Set` / tuples | 🔲 M3 | |
| Null safety / optionals (`T?`) | ✅ | `??`, `?.`, `if (var x = opt)`, checked `opt!`, `match` over `T?`; non-optional `T` is never null (compile-time) |
| Pipe operator `\|>` | ✅ | `x \|> f ≡ f(x)`; left-associative, lowered to a call in the parser; transpiles to a plain PHP call |
| Exceptions (try/catch/throw) | 🔲 M3 | |
| Mutation (reassignment, field writes) | 🔲 M3 | triggers the tracing GC |
| Traits, operator overloading, method overloading | 🔲 future | |
| Modules / packages | 🚧 M5 | multi-file projects, folder=path, cross-package `import` + aliasing, namespaced PHP, **git dependencies** (`[require]` + `phg vendor` + `phorge.lock`, offline) — shipped; transitive deps next |
| Concurrency (`spawn` + channels) | 🔲 M6 | uncolored, green-threaded |

## Backends & tooling

| Capability | Status | Command |
|---|---|---|
| Tree-walking interpreter (reference semantics) | ✅ | `phg run` |
| Bytecode compiler + stack VM (byte-identical) | ✅ | `phg runvm` |
| Backend benchmark (median-of-N, identity-gated) + memory (peak/current RSS, Linux) | ✅ | `phg bench` |
| Bytecode disassembler (per-function listings + descriptor tables) | ✅ | `phg disasm` |
| Phorge → PHP transpiler (runs under real PHP) | ✅ | `phg transpile` |
| Type-check / parse / lex inspection | ✅ | `phg check` / `parse` / `lex` |
| `--version` / `--help`, plus per-command help with examples | ✅ | `phg -v` / `-h` / `phg <cmd> --help` |
| Sharp diagnostics: caret-underlined span, did-you-mean hints, stable codes | ✅ | front-end errors |
| Diagnostic dictionary (look up a code) | ✅ | `phg explain <CODE>` |
| Program from stdin / inline / `--` | ✅ | `run -`, `run -e '…'`, `run -- <file>` |
| Vendor git dependencies (offline, lockfile-pinned) | ✅ | `phg vendor` |
| HTTP server: `handle(Request) -> Response` (pure Phorge) over a real socket; PHP `php -S` bridge | ✅ | `phg serve foo.phg` |
| Standalone executable (host) | ✅ | `phg build foo.phg` |
| Standalone executable (Linux cross + Windows) | 🔨 | `phg build --target … / --all` |
| Standalone executable (macOS) | 🔲 | reader ships; signed stub deferred to M2.5 Phase 3 |
| PHP → Phorge migration | 🔲 M8 | the inverse of the transpiler |
| Editor/LSP, formatter | 🔲 M7 | |

## Project qualities

- **Zero external runtime dependencies** — std-only Rust, nothing to download (see
  [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)).
- **No `unsafe`** — `#![forbid(unsafe_code)]` crate-wide.
- **Never panics on input** — adversarial source *and* adversarial binaries are handled cleanly
  (invariant EV-7).
- **Differential-tested** — every example runs on both backends and must match byte-for-byte.
