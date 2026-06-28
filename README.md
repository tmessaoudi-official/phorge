# Phorge

[![CI](https://github.com/tmessaoudi-official/phorge/actions/workflows/ci.yml/badge.svg)](https://github.com/tmessaoudi-official/phorge/actions/workflows/ci.yml)

**A statically-typed, PHP-inspired programming language — implemented in Rust, std-only, with
zero external crates.**

Phorge takes the ergonomics that make PHP pleasant to write (familiar syntax, string interpolation,
classes) and puts them on a **statically-typed, immutable-by-default** footing with a clean compiler
pipeline. It runs on **two byte-identical backends** — a tree-walking interpreter and a bytecode
stack VM — transpiles to **real PHP**, and can compile a program into a **single standalone native
executable** with no runtime to install.

Phorge is built to grow into a **full, general-purpose language** — aiming to match the breadth that
makes PHP productive (and then some), not a toy DSL. **Performance is a first-class goal:** programs
run on a bytecode VM, and an early three-way benchmark (`phg bench --vs-php`) already shows the VM
ahead of PHP on a sample workload — with rigorous, comprehensive benchmarks a tracked milestone on
the road to GA.

```phorge
package Main;
import Core.Console;

enum Shape {
    Circle(float radius),
    Rect(float w, float h),
}

function area(Shape s): float {
    return match s {
        Circle(r)  => 3.14159 * r * r,
        Rect(w, h) => w * h,
    };
}

function main() {
    List<Shape> shapes = [Circle(2.0), Rect(3.0, 4.0)];
    for (Shape s in shapes) {
        Console.println("area = {area(s)}");
    }
}
```

---

## Status

| Milestone | What | State |
|---|---|---|
| **M1** | Tree-walking interpreter + Phorge→PHP transpiler | ✅ complete |
| **M2** | Bytecode compiler + stack VM (byte-identical to the interpreter) | ✅ complete |
| **M2.5** | `phg build` → standalone native executables | 🔨 in progress (Linux + Windows cross-builds; macOS readers shipped, stub deferred) |
| **M3+** | Language enrichment, ecosystem, tooling | 🔲 planned — see [ROADMAP.md](ROADMAP.md) & [VISION.md](VISION.md) |

Pre-1.0 and single-developer; the version number tracks milestone progress, not a release cadence.
Full status lives in [`docs/MILESTONES.md`](docs/MILESTONES.md).

## How it works

```
source .phg
  │  lexer        (&str → tokens)
  │  parser       (recursive descent → AST)           depth-guarded
  │  checker      (type-check gate; validates, does not annotate)
  ▼
validated AST
  ├─▶ interpreter      tree-walker        → stdout   ┐ the reference semantics (the oracle)
  ├─▶ compiler → VM    bytecode stack VM  → stdout   │ byte-identical to the interpreter
  ├─▶ transpiler       AST → PHP source   → stdout   ┘ runs under real PHP, byte-identical
  └─▶ build            embed source in a native binary that self-runs on the VM
```

The interpreter is the **reference semantics**; the VM must match it **byte-for-byte**, enforced by a
differential test harness (`tests/differential.rs`) that runs every example through both backends. A
standalone built binary is a third surface on the same spine: it runs its embedded source on the VM
and must produce identical output. See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the module
map and [`docs/INVARIANTS.md`](docs/INVARIANTS.md) for the rules that keep all surfaces in lock-step.

## Install

### From source

```sh
cargo build --release        # produces target/release/phg
cargo test                   # full suite
cargo clippy --all-targets   # lints (warnings are denied)
```

Toolchain: Rust (edition 2021), std-only — **no external crates** to download.

### Prebuilt binary

Standalone, statically-linked binaries (no runtime to install) are published per release. Grab the
one for your platform, mark it executable, and run:

```sh
chmod +x phg-*-linux-x86_64-musl
./phg-*-linux-x86_64-musl run yourfile.phg
```

## Quick start

```sh
$ phg run examples/hello.phg
Hello, Phorge!

$ echo 'package Main; import Core.Console; function main() { Console.println("{1 + 2}"); }' | phg run -
3

$ phg run -e 'package Main; import Core.Console; function main() { Console.println("inline!"); }'
inline!
```

## CLI

```
phg <command> <source> [options]
```

**Commands** (each is a stage of the pipeline):

| Command | Does | On error |
|---|---|---|
| `run` | lex → parse → type-check → interpret (tree-walker) | exit 1 |
| `runvm` | lex → parse → type-check → compile → stack VM | exit 1 |
| `check` | type-check only, report success | exit 1 on type error |
| `parse` | dump the AST | exit 1 on parse error |
| `lex` | dump the token stream | exit 1 on lex error |
| `transpile` | type-check (gate) → emit PHP to stdout | exit 1 on type/transpile error |
| `disasm` | type-check → compile → dump the bytecode (per-function listings + descriptor tables) | exit 1 on type error |
| `bench` | median-of-N timing of both backends + memory (peak/current RSS, Linux), output-identity gated | exit 1 if they fault or disagree |
| `build` | compile to a standalone native executable | exit 1 on type error / build failure |
| `vendor` | fetch + pin git dependencies into `vendor/` (the only network-touching command), writing `phorge.lock` | exit 1 on fetch/lock failure |
| `serve` | run an HTTP server that dispatches requests to a Phorge `handle(Request) -> Response` (M6) | exit 1 on bind/handler error |
| `test` | discover + run `test "name" { … }` blocks (under `tests/`, or a given file/dir) with `Core.Test` assertions | exit 1 if any test fails |
| `fmt` | format source to canonical form (`--check` for CI, `-` for stdin, in-place otherwise) | `--check`: exit 1 if any file would change; exit 2 on a parse error |
| `explain` | look up a diagnostic code (`phg explain E-UNKNOWN-IDENT`) | exit 1 on unknown code |

**Source** (for the run-family commands):

| Form | Reads the program from |
|---|---|
| `<file>` | a file path |
| `-` | standard input |
| `-e <code>` / `--eval <code>` | inline source text |
| `-- <file>` | a file path that may start with `-` |

**Global flags:** `-h` / `--help` (full usage; `phg <command> --help` gives per-command help with
worked examples) · `-v` / `--version`.

No arguments → usage on stderr, exit 2. Unreadable file → exit 1.

## Standalone executables (`phg build`)

`phg build foo.phg` produces a native executable that runs `foo.phg` on the VM with **no Phorge
install** required. The program source is embedded in a named object-file section (a versioned,
CRC-guarded container); at startup the binary detects and runs it.

```sh
phg build foo.phg -o foo                     # host build
phg build foo.phg --target x86_64-unknown-linux-musl -o foo-musl
phg build foo.phg --all                      # host + all supported cross-targets → dist/
```

Cross-compilation uses [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild) (zig as the
linker) plus `llvm-objcopy`. Supported targets today: `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-{gnu,musl}`, `x86_64-pc-windows-gnu`. The Mach-O/macOS section reader ships and
is tested, but macOS *stub production* (signing) is deferred to a later phase — apple targets are
rejected with a clear message. Cross-builds require a phorge source checkout (the host build does
not). See [ROADMAP.md](ROADMAP.md) for Phase 2/3 details.

## Testing (`phg test`)

Write tests *in Phorge* and run them with one command:

```phorge
package Main;
import Core.Test;

function add(int a, int b): int { return a + b; }

test "addition" {
    Test.assertEquals(add(2, 3), 5);
    Test.assertTrue(1 < 2);
}
```

```console
$ phg test            # every *.phg under tests/ (or: phg test <file|dir>)
... :: addition ... ok
1 passed, 0 failed, 1 tests in 1 files
```

A `test "name" { … }` block is checked like a `-> void` body and runs on the interpreter; a failing
assertion (or any other fault) is reported with its message, line, and stack trace, and the runner
continues. Exit code is `0` iff every test passes — so `phg test` drops straight into CI. The
`Core.Test` assertions are `assert`, `assertTrue`/`assertFalse`, `assertEquals`/`assertNotEquals`,
`assertNull`/`assertNotNull`, and `assertFaults(() -> T)` (passes iff the closure faults). A runnable
showcase lives in [`selftest/`](selftest/README.md).

## Formatting (`phg fmt`)

`phg fmt` rewrites source to a canonical form (`gofmt`/`rustfmt` shaped):

```sh
phg fmt                 # format every *.phg under the current directory, in place
phg fmt src/app.phg     # one file
phg fmt --check .       # CI gate: exit 1 if anything isn't formatted, write nothing
cat app.phg | phg fmt - # stdin → stdout
```

It is **meaning-preserving by construction** — it prints from the parsed AST (not by re-spacing
tokens), so formatting can never change what a program means; it is idempotent, and an unparseable
file is left untouched (its diagnostic is reported, exit 2). Comments are preserved. v1 is *tidy +
comment-safe* (canonical indentation, spacing, blank-line collapse, `->`→`:` return syntax); line
wrapping/width-reflow is a later addition.

## Editor support (`phg lsp`)

`phg lsp` is a [Language Server](https://microsoft.github.io/language-server-protocol/) over stdio, so
any LSP-capable editor gets:

- **Diagnostics** as you type — the *same* checker `phg check` runs, so editor squiggles equal the CLI
  (highlighting the whole offending token).
- **Hover** — the declaration signature of the symbol under the cursor (top-level *or* a local/param).
- **Go-to-definition** — jump to a function / class / enum / interface / trait / type declaration, or to
  a local binding (parameter, `var`, `for` variable, `if`-let, `catch`, destructure) in scope.
- **Completion** — top-level names, in-scope locals/params, and keywords.
- **Document symbols** — a structured outline (classes/enums/interfaces/traits expand to their members).

It is dependency-free (hand-rolled JSON-RPC in `std` — no `tower-lsp`/`serde`), like the rest of Phorge.

**VS Code**: a thin client lives in [`editors/vscode/`](editors/vscode/) — see its README to run it in an
Extension Development Host or package it. **Other editors** (Neovim, Helix, PhpStorm, …): register a
language server for `*.phg` that launches `phg lsp` with stdio transport. For example, Neovim:

```lua
vim.lsp.start({ name = 'phorge', cmd = { 'phg', 'lsp' }, root_dir = vim.fn.getcwd() })
```

## Language at a glance

- **Static types** — `int`, `float`, `bool`, `string`, generic `List<T>`.
- **Local type inference** — `var x = expr;` infers the binding type from its initializer (still
  fully static, still immutable).
- **Type aliases** — `type UserId = int;` names a type for readability; compile-time only, erased in
  the transpiled PHP.
- **Immutable by default** — no reassignment; introduce a fresh binding (`int y = x + 1;`).
- **Functions** — `function f(int n): int { ... }`; `main()` is the entry point.
- **Classes** — with **constructor promotion** (`constructor(private int total) {}` declares and
  assigns the field in one place), fields, and instance methods (`this`).
- **Enums** — algebraic data types with payloads:
  `enum Shape { Circle(float radius), Rect(float w, float h) }`.
- **`match`** — exhaustiveness-checked pattern matching over enum variants.
- **String interpolation** — `"area = {area(s)}"`.
- **`for ... in`** over lists — `for (int s in [80, 30, 55]) { ... }`.
- **Indexing** — `xs[i]` reads a list element by position; an out-of-range read is a clean runtime
  fault, never a silent wrong value.
- **Integer ranges** — `0..n` (exclusive) and `0..=n` (inclusive), mainly for `for (int i in 0..n)`.
- **Expression `if`** — `if (c) { e } else { e }` yields a value: `var x = if (c) { 1 } else { 2 };`.
- **Checked arithmetic** — int overflow and division-by-zero are clean runtime errors, never panics.
- **Sharp diagnostics** — type errors underline the offending span with a caret, suggest the nearest
  in-scope name on a typo, and carry a stable code you can look up with `phg explain <CODE>`.

A full capability matrix (implemented vs. planned) lives in [FEATURES.md](FEATURES.md); current
limitations in [KNOWN_ISSUES.md](KNOWN_ISSUES.md); the frozen language design in
`docs/specs/2026-06-15-phorge-language-design.md`.

## Examples

Every program under [`examples/`](examples/README.md) runs byte-identically on both backends (gated by
`tests/differential.rs`, which globs the directory — a new example is auto-gated the moment it lands).
`examples/realworld/` holds four real programs (a ledger, a shop, an RPG, a small library);
`examples/guide/` holds focused tours of each feature.

## Phorge → PHP transpiler

`phg transpile <file>` emits PHP 8.x (type-checked first): enums → an abstract base class plus a
`final` subclass per variant; `match` → an `instanceof` chain; interpolation → concatenation;
`println` → `echo`. The round-trip is verified against a real `php` in `tests/cli.rs`. (PHP → Phorge
import is a separate future milestone.)

## Roadmap & vision

- **[ROADMAP.md](ROADMAP.md)** — milestone-by-milestone plan from here to a full ecosystem.
- **[VISION.md](VISION.md)** — what Phorge is *for*, and the long-term ambition.
- **[docs/MILESTONES.md](docs/MILESTONES.md)** — living status with commit references.

## Contributing

Contributions are welcome — see **[CONTRIBUTING.md](CONTRIBUTING.md)** for the dev setup, the quality
gate, the test-driven workflow, and the correctness invariants you must preserve. By participating you
agree to the **[Code of Conduct](CODE_OF_CONDUCT.md)**. To report a security issue, see
**[SECURITY.md](SECURITY.md)**. For help, see **[SUPPORT.md](SUPPORT.md)**.

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in Phorge by you, as defined in the Apache-2.0 license, shall be dual-licensed as above,
without any additional terms or conditions. Phorge has **no third-party runtime dependencies** — see
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
