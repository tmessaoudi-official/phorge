# Phorge

**A small, statically-typed, PHP-inspired programming language — implemented in Rust, std-only, with
zero external crates.**

Phorge takes the ergonomics that make PHP pleasant to write (familiar syntax, string interpolation,
classes) and puts them on a **statically-typed, immutable-by-default** footing with a clean compiler
pipeline. It runs on **two byte-identical backends** — a tree-walking interpreter and a bytecode
stack VM — transpiles to **real PHP**, and can compile a program into a **single standalone native
executable** with no runtime to install.

```phorge
import std.io;

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

function main() {
    List<Shape> shapes = [Circle(2.0), Rect(3.0, 4.0)];
    for (Shape s in shapes) {
        println("area = {area(s)}");
    }
}
```

---

## Status

| Milestone | What | State |
|---|---|---|
| **M1** | Tree-walking interpreter + Phorge→PHP transpiler | ✅ complete |
| **M2** | Bytecode compiler + stack VM (byte-identical to the interpreter) | ✅ complete |
| **M2.5** | `phorge build` → standalone native executables | 🔨 in progress (Linux + Windows cross-builds; macOS readers shipped, stub deferred) |
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
cargo build --release        # produces target/release/phorge
cargo test                   # full suite
cargo clippy --all-targets   # lints (warnings are denied)
```

Toolchain: Rust (edition 2021), std-only — **no external crates** to download.

### Prebuilt binary

Standalone, statically-linked binaries (no runtime to install) are published per release. Grab the
one for your platform, mark it executable, and run:

```sh
chmod +x phorge-*-linux-x86_64-musl
./phorge-*-linux-x86_64-musl run yourfile.phg
```

## Quick start

```sh
$ phorge run examples/hello.phg
Hello, Phorge!

$ echo 'function main() { println("{1 + 2}"); }' | phorge run -
3

$ phorge run -e 'function main() { println("inline!"); }'
inline!
```

## CLI

```
phorge <command> <source> [options]
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

**Source** (for the run-family commands):

| Form | Reads the program from |
|---|---|
| `<file>` | a file path |
| `-` | standard input |
| `-e <code>` / `--eval <code>` | inline source text |
| `-- <file>` | a file path that may start with `-` |

**Global flags:** `-h` / `--help` (full usage) · `-v` / `--version`.

No arguments → usage on stderr, exit 2. Unreadable file → exit 1.

## Standalone executables (`phorge build`)

`phorge build foo.phg` produces a native executable that runs `foo.phg` on the VM with **no Phorge
install** required. The program source is embedded in a named object-file section (a versioned,
CRC-guarded container); at startup the binary detects and runs it.

```sh
phorge build foo.phg -o foo                     # host build
phorge build foo.phg --target x86_64-unknown-linux-musl -o foo-musl
phorge build foo.phg --all                      # host + all supported cross-targets → dist/
```

Cross-compilation uses [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild) (zig as the
linker) plus `llvm-objcopy`. Supported targets today: `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-{gnu,musl}`, `x86_64-pc-windows-gnu`. The Mach-O/macOS section reader ships and
is tested, but macOS *stub production* (signing) is deferred to a later phase — apple targets are
rejected with a clear message. Cross-builds require a phorge source checkout (the host build does
not). See [ROADMAP.md](ROADMAP.md) for Phase 2/3 details.

## Language at a glance

- **Static types** — `int`, `float`, `bool`, `string`, generic `List<T>`.
- **Immutable by default** — no reassignment; introduce a fresh binding (`int y = x + 1;`).
- **Functions** — `function f(int n) -> int { ... }`; `main()` is the entry point.
- **Classes** — with **constructor promotion** (`constructor(private int total) {}` declares and
  assigns the field in one place), fields, and instance methods (`this`).
- **Enums** — algebraic data types with payloads:
  `enum Shape { Circle(float radius), Rect(float w, float h) }`.
- **`match`** — exhaustiveness-checked pattern matching over enum variants.
- **String interpolation** — `"area = {area(s)}"`.
- **`for ... in`** over lists — `for (int s in [80, 30, 55]) { ... }`.
- **Checked arithmetic** — int overflow and division-by-zero are clean runtime errors, never panics.

A full capability matrix (implemented vs. planned) lives in [FEATURES.md](FEATURES.md); current
limitations in [KNOWN_ISSUES.md](KNOWN_ISSUES.md); the frozen language design in
`docs/specs/2026-06-15-phorge-language-design.md`.

## Examples

Every program under [`examples/`](examples/README.md) runs byte-identically on both backends (gated by
`tests/differential.rs`, which globs the directory — a new example is auto-gated the moment it lands).
`examples/realworld/` holds four real programs (a ledger, a shop, an RPG, a small library);
`examples/guide/` holds focused tours of each feature.

## Phorge → PHP transpiler

`phorge transpile <file>` emits PHP 8.x (type-checked first): enums → an abstract base class plus a
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
