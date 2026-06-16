# Phorge

A small, statically-typed, PHP-inspired programming language implemented in Rust
(std-only, no external crates).

**Status — M1 complete; M2 P1–P3 in progress:** a tree-walking interpreter (lexer → parser →
type-checker → evaluator) plus a **Phorge → PHP transpiler** (M1). M2 adds a bytecode compiler and
stack VM (`phorge runvm`), byte-identical to the interpreter across the full M1 surface (M2 P4)
and differential-tested; `phorge bench` measures the VM against the tree-walker. The emitted PHP runs
under a real `php` and produces byte-identical output to the interpreter.

## Build

```sh
cargo build --release        # produces target/release/phorge
cargo test                   # full suite
cargo clippy --all-targets   # lints
```

Toolchain: Rust (edition 2021).

## Quickstart

```sh
$ phorge run examples/hello.phg
Hello, Phorge!
```

`examples/hello.phg`:

```phorge
import std.io;

function main() {
    println("Hello, Phorge!");
}
```

## CLI

`phorge <command> <file>` — seven commands, each a stage of the pipeline (plus `bench`):

| Command | Does | On error |
|---|---|---|
| `run` | lex → parse → type-check → interpret (tree-walker) | exit 1, error on stderr |
| `runvm` | lex → parse → type-check → compile to bytecode → VM (M2) | exit 1, error on stderr |
| `check` | lex → parse → type-check, report only | exit 1 on type error |
| `parse` | lex → parse, dump the AST | exit 1 on parse error |
| `lex` | dump the token stream | exit 1 on lex error |
| `transpile` | type-check (gate) → emit PHP to stdout | exit 1 on type/transpile error |
| `bench` | median-of-N timing of both backends, output-identity gated (M2) | exit 1 if a backend faults or they disagree |

`runvm` is the M2 bytecode backend: identical output to `run`, executed on a stack VM
instead of the tree-walker. The two are kept in lock-step by the differential test harness
(`tests/differential.rs`). As of **M2 P4**, the VM covers the **full M1 language surface** —
expressions/statements, functions + recursion, single-payload enums + exhaustive `match`, and
classes (constructor promotion, field reads, instance methods + `this`). `examples/fib.phg` and
`examples/grades.phg` both run byte-identically on the VM; `phorge bench` shows it outpacing the
tree-walker.

No arguments → usage on stderr, exit 2. Unreadable file → exit 1.

```sh
$ phorge check examples/hello.phg
OK (type-checks clean)

$ phorge transpile examples/hello.phg
<?php
function main(): void {
    echo "Hello, Phorge!" . "\n";
}
main();

$ phorge lex examples/hello.phg | head -4
Import @ 1:1
Ident("std") @ 1:8
Dot @ 1:11
Ident("io") @ 1:12
```

## Language at a glance

- **Static types** — `int`, `float`, `bool`, `string`, generic `List<T>`.
- **Immutable** — no reassignment; introduce a fresh binding (`int y = x + 1;`).
- **Functions** — `function f(int n) -> int { ... }`; a `main()` is the entry point.
- **Classes** — with **constructor promotion** (`constructor(private int total) {}`
  declares and assigns the field in one place).
- **Enums** — algebraic data types with payloads: `enum Shape { Circle(float radius), Rect(float w, float h) }`.
- **`match`** — exhaustiveness-checked pattern matching over enum variants.
- **String interpolation** — `"area = {area(s)}"`.
- **`for ... in`** over list literals — `for (int s in [80, 30, 55]) { ... }`.

See `docs/specs/2026-06-15-phorge-language-design.md` for the full design.

## Examples

Every program under `examples/` is a runnable Phorge program (guarded by
`tests/examples.rs`, which runs each one and asserts a clean exit):

- `hello.phg` — minimal hello-world.
- `fib.phg` — recursion (Fibonacci).
- `grades.phg` — enums, `if`, `match`, a class with constructor promotion, `for ... in`.

## Phorge → PHP transpiler

`phorge transpile <file>` emits PHP 8.x source (type-checked first). Mappings: enums →
an abstract base class plus a `final` subclass per variant (promoted public props);
`match` → an `instanceof` chain; string interpolation → PHP concatenation; `println` →
`echo`. The round-trip is verified against a real `php` in `tests/cli.rs`.

PHP → Phorge import is **not** part of M1 (it needs a full PHP front-end plus
dynamic → static type inference — a separate milestone).

## Known limitations (M1)

Out of scope for the tree-walking interpreter; these are rejected cleanly (type or
transpile error) rather than crashing: nullable types / `null`, the `|>` pipe operator,
the `is` operator, indexing, `Map`/`Set`, function overloading, and `match` outside
return / variable-declaration-initializer position. Recursion uses the native Rust call
stack, so pathological depth overflows.
