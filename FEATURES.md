# Features

A capability matrix for Phorge — what works **today** versus what is **planned**. For runnable proof
of the "today" column, see [`examples/`](examples/README.md); for the forward plan see
[ROADMAP.md](ROADMAP.md); for things that are deliberately rejected-but-clean, see
[KNOWN_ISSUES.md](KNOWN_ISSUES.md).

## Language

| Feature | Status | Notes |
|---|---|---|
| Static types: `int`, `float`, `bool`, `string` | ✅ | checked at compile time |
| Raw bytes: `bytes` + `b"…"` literals (`\xHH`) | ✅ | octet sequences distinct from UTF-8 `string`; `core.bytes` interop (`from_string`/`to_string`/`len`/`concat`/`slice`) |
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
| `Map` / `Set` / tuples | 🔲 M3 | |
| Null safety / optionals (`T?`) | ✅ | `??`, `?.`, `if (var x = opt)`, checked `opt!`, `match` over `T?`; non-optional `T` is never null (compile-time) |
| Pipe operator `\|>` | 🔲 M3 | |
| Exceptions (try/catch/throw) | 🔲 M3 | |
| Mutation (reassignment, field writes) | 🔲 M3 | triggers the tracing GC |
| Traits, operator overloading, method overloading | 🔲 future | |
| Modules / packages | 🚧 M5 | multi-file projects, folder=path, cross-package `import` + aliasing, namespaced PHP, **git dependencies** (`[require]` + `phorge vendor` + `phorge.lock`, offline) — shipped; transitive deps next |
| Concurrency (`spawn` + channels) | 🔲 M6 | uncolored, green-threaded |

## Backends & tooling

| Capability | Status | Command |
|---|---|---|
| Tree-walking interpreter (reference semantics) | ✅ | `phorge run` |
| Bytecode compiler + stack VM (byte-identical) | ✅ | `phorge runvm` |
| Backend benchmark (median-of-N, identity-gated) + memory (peak/current RSS, Linux) | ✅ | `phorge bench` |
| Bytecode disassembler (per-function listings + descriptor tables) | ✅ | `phorge disasm` |
| Phorge → PHP transpiler (runs under real PHP) | ✅ | `phorge transpile` |
| Type-check / parse / lex inspection | ✅ | `phorge check` / `parse` / `lex` |
| `--version` / `--help`, plus per-command help with examples | ✅ | `phorge -v` / `-h` / `phorge <cmd> --help` |
| Sharp diagnostics: caret-underlined span, did-you-mean hints, stable codes | ✅ | front-end errors |
| Diagnostic dictionary (look up a code) | ✅ | `phorge explain <CODE>` |
| Program from stdin / inline / `--` | ✅ | `run -`, `run -e '…'`, `run -- <file>` |
| Vendor git dependencies (offline, lockfile-pinned) | ✅ | `phorge vendor` |
| Standalone executable (host) | ✅ | `phorge build foo.phg` |
| Standalone executable (Linux cross + Windows) | 🔨 | `phorge build --target … / --all` |
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
