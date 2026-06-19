# Changelog

All notable changes to Phorge. Format follows [Keep a Changelog](https://keepachangelog.com/);
the project is pre-1.0 and unpublished, so versions track milestone progress, not a release
cadence. Milestones and their status live in `docs/MILESTONES.md`.

## [Unreleased]

### core.html — typed auto-escaping HTML (Waves 1–2: escape kernel + element builders)

- **Wave 2 — typed element builders.** A new distinct type `Attr` (like `Html`, erases to PHP
  `string`, non-interchangeable) plus five `core.html` natives compose HTML from typed fragments
  rather than hand-written markup: `attr(string, string) -> Attr` (value escaped, name trusted),
  `bool_attr(string) -> Attr` (valueless), `el(string, List<Attr>, List<Html>) -> Html`,
  `void_el(string, List<Attr>) -> Html` (self-closing), and `concat(List<Html>) -> Html`. Each
  builder's `eval` and its PHP emission are held byte-identical by a unit test (the `el`/`void_el`
  PHP uses an IIFE so the tag expression evaluates exactly once). No new `Op`; the safety wall and
  zero runtime divergence carry over from Wave 1. `examples/guide/html.phg` now also exercises the
  builders, byte-identical on `run`/`runvm`/**real PHP**.
- **Empty list literal `[]` as a call argument** now adopts its element type from the expected
  parameter type (a small, call-argument-only bit of bidirectional checking in `check_args`), so a
  zero-attribute or zero-child builder call reads naturally — `el("p", [], [text(x)])`. An empty
  `[]` in a declaration initializer or `return` still requires a non-empty literal.
- **`Html` type + `core.html` escape kernel (Wave 1).** The Phorge-idiomatic answer to "how do I write HTML"
  (design: `docs/specs/2026-06-19-core-html-design.md`). `Html` is a distinct checker type
  (`Ty::Html`) that erases to PHP `string` and rides `Value::Str` at runtime — but is **not
  interchangeable with `string`**, so untrusted text cannot reach rendered HTML except through
  `core.html.text` (auto-escape) or the audited `core.html.raw` (trusted markup). This makes XSS a
  *compile error*, not a runtime hazard — enforced by the type checker, zero new `Op`, zero runtime
  divergence. Boundary natives: `text(string) -> Html`, `raw(string) -> Html`, `render(Html) ->
  string`. Escaping erases to the **pinned** `htmlspecialchars($s, ENT_QUOTES, 'UTF-8')` (tier-1,
  `php -n`-safe) and is mirrored by a Rust five-char table held byte-identical by a unit test.
  `examples/guide/html.phg` runs byte-identically on `run`/`runvm`/**real PHP**. (Builders shipped in
  Wave 2, above; the `html"…"` literal sugar is Wave 3.)

### M9 — Engineering Hygiene (CI enforcement)

- **GitHub Actions CI (`.github/workflows/ci.yml`) — locks in M7.** A `gate` job runs the same three
  checks as the local pre-commit hook (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`) on the toolchain pinned in `rust-toolchain.toml`, and sets `PHORGE_REQUIRE_PHP=1` (with
  `php` installed via `setup-php`) so the M7 PHP oracle in `tests/differential.rs` **fails** rather than
  skips if transpiled PHP diverges from the interpreter/VM. A `cross-build` job installs Zig +
  `cargo-zigbuild` + the four Phase-2 cross targets + `llvm-objcopy` (from `llvm-tools-preview`, via
  `PHORGE_OBJCOPY`) and runs `tests/build.rs` for real (x86_64-musl native exec + windows-gnu PE
  round-trip), plus an aarch64-gnu/musl compile smoke. This makes CONTRIBUTING.md's "CI runs the same
  gate" true (no workflow existed before).

### M7 — Correctness Closure (the third backend leg, enforced)

The transpiler→PHP backend is now inside the automated correctness loop. Previously
`tests/differential.rs` gated only `run ≡ runvm`; the transpiled PHP was never executed, so
transpiler→PHP divergences shipped silently — including inside examples advertising three-way
byte-identity.

- **PHP oracle (closes P0-ROOT).** `tests/differential.rs` gains `all_examples_transpile_and_match_php`
  and `all_example_projects_transpile_and_match_php`: every runnable example/project is transpiled,
  executed by a real `php`, and its stdout asserted byte-identical to the interpreter's (⇒ all three
  backends identical, since `run ≡ runvm` is already gated). **Fails-not-skips:** `PHORGE_REQUIRE_PHP=1`
  makes a missing `php` a test **failure** (CI mode); unset, it skips *loudly* (logged), never a silent
  green. `PHORGE_PHP=<path>` overrides the binary. Examples using a not-yet-transpiled construct are
  loudly deferred (logged `DEFER`, counted), not silently passed. The two narrow self-skipping PHP
  round-trip tests in `tests/cli.rs` (and their if-let/opt!/match-optional siblings — five in all) are
  removed, subsumed by the oracle.
- **P0-1 — integer division.** `7 / 2` now transpiles to `__phorge_div(7, 2)` (a runtime helper:
  `is_int($a)&&is_int($b) ? intdiv : /`), matching Phorge's truncate-toward-zero integer `/`. PHP's
  always-float `/` previously made `7/2` print `3.5` instead of `3`, live in `operators.phg`.
- **P0-4 — float modulo.** `5.5 % 2.0` transpiles to `__phorge_rem(…)` (`is_int…? % : fmod`), matching
  Phorge's `fmod`-style float `%`. PHP's integer `%` previously printed `1` instead of `1.5`.
- **P0-3 — bool interpolation.** An interpolated value is coerced via `__phorge_str` (`is_bool ?
  "true"/"false" : (string)$v`), mirroring `Value::as_display`. PHP's bool-in-string previously printed
  `1`/`` (empty) instead of `true`/`false`, live in `control-flow.phg`/`operators.phg`.
- **P0-2 — operand grouping.** Compound operands of unary/binary ops are now parenthesized
  (`a - (b - c)` → `$a - ($b - $c)`, `!(a && b)` → `!($a && $b)`), so PHP precedence can't
  re-associate them.
- **QW-13 — empty/reversed ranges.** Ranges transpile through `__phorge_range($a, $b, $inclusive)`,
  which yields `[]` for an empty/reversed range (PHP's bare `range()` descends). The KNOWN_ISSUES
  caveat is removed.
- **P1-#9 — large ranges fault cleanly.** A range wider than the new single-sourced
  `value::MAX_RANGE_LEN` (10M) now faults `"range too large"` (classified `FaultKind::RangeTooLarge`,
  `agree_err`-gated on both backends) instead of OOM-aborting (exit 101). Length is computed with
  `checked_sub` (EV-7). `value::build_range` single-sources the size-guarded materialization for both
  backends.

The four P0 fixes use runtime PHP helpers (mirroring Phorge's type-driven value kernels) rather than a
transpiler-side static type resolver — no duplicated operand-type inference, no inference-completeness
risk. `run ≡ runvm` was always correct; the bug class was php-leg-only.

### M3 S3 (Track A) — lambdas, first-class functions, and the pipe operator

- **Lambdas / closures.** `fn(int x) => x * 2` (expression body, return type inferred) and
  `fn(int x) -> int { … }` (statement body, explicit `-> T` required, `E-LAMBDA-THIS` if it touches
  `this`). Free enclosing locals are captured **by value** (the heap is immutable + acyclic, so no GC
  is needed). New surfaces: `Ty::Function` / `Type::Function`, `Expr::Lambda` + `LambdaBody`,
  `ast::free_vars`, `Value::Closure`, `CTy::Fn`, and two VM ops `Op::MakeClosure` / `Op::CallValue`.
- **First-class function values.** A bare named function is a value — `twice(3, dbl)` passes `dbl`
  itself; the function type is `(int) -> int`. On the VM a named-fn reference compiles to a
  zero-capture `MakeClosure`; the transpiler emits a PHP first-class callable `dbl(...)`.
- **Pipe operator `|>`.** `x |> f ≡ f(x)`, left-associative, **lowered to a plain call in the
  parser** (no new `Op`, no new backend semantics; the four dead `BinaryOp::Pipe` stubs are retired
  to `unreachable!`). `5 |> dbl |> inc` is `inc(dbl(5))`; `1 + 2 |> dbl` is `dbl(1 + 2)`.
- **Transpile targets** (Phorge : PHP :: TypeScript : JavaScript): expression lambda → arrow fn
  `fn($x) => …`; statement lambda → `function($x) use ($cap) { … }` (by-value `use`); named-fn ref →
  first-class callable; a lambda literal in call position → `(fn(…) => …)(args)`.
- All byte-identical on `run`/`runvm` and round-tripped through real PHP 8.6. Example:
  `examples/guide/lambdas-pipe.phg`. Deferred refinements (this-capture, cross-package value refs,
  block-body return inference, function-type variance, `core.list` map/filter/reduce) are recorded in
  `KNOWN_ISSUES.md`.

### M6 slices W2–W4 — routing, the serve runtime, and `phg serve`

- **W2 — static router (pure Phorge, no new feature).** A data-driven `List<Route>` table is scanned
  linearly for an exact `(method, path)` match, yielding a `Handler` enum tag dispatched by an
  exhaustive `match` to named handler functions; a method-sensitive 404 fallback. Routing is fully
  expressible with today's enums + classes + lists + `match`, so it is byte-identical on `run`/`runvm`
  and round-trips through real PHP. Example: `examples/web/router.phg`.
- **W3 — the serve runtime (`src/serve.rs`), the determinism quarantine.** The one module holding
  sockets + wall-clock non-determinism, deliberately **outside** `tests/differential.rs`. A `Transport`
  trait (`recv`/`send`) seams the loop from the world; `TcpTransport` is the real single-threaded
  socket (`Connection: close`, CRLFCRLF + `Content-Length` framing capped at 8 MiB, EV-7 no-panic).
  `serve()` routes each raw buffer through the program's single entry `respond(bytes) -> bytes`,
  degrading a request fault to a 500. **Single-threaded by force** — the `Rc`-shared heap makes runtime
  values non-`Send`, so a thread pool is impossible; true concurrency awaits M6 green-threads under the
  unchanged contract.
- **`interpreter::call_named(program, name, args)`** — invoke a named top-level function with a
  constructed argument (reuses `run_call`). The interpreter is the reference backend and `run ≡ runvm`
  guarantees the VM would agree, so a VM `call_named` (no return-value capture today) is deferred. No
  new `Op`, no new `Value` variant.
- **W4 — `phg serve <file> [--addr 127.0.0.1:8080]`.** Loads the program project-aware (like `run`),
  type-checks it, then runs the blocking HTTP serve loop on the 256 MB deep-stack worker (so the
  interpreter's `MAX_CALL_DEPTH` guard has the same headroom `run`/`runvm` rely on). Per-command
  `--help` with worked examples. Built binaries still ignore argv.
- **PHP bridge (`php -S`).** `examples/web/server.php` is a hand-written front-controller that builds a
  `Request` from PHP superglobals and calls the *transpiled* `handle(Request) -> Response` — the same
  value unit `phg serve` calls natively. The superglobal↔`Request` adapter is runtime glue, not
  transpiled (mirroring `src/serve.rs`). Documented end-to-end in `examples/web/README.md`.
- **Example** `examples/web/server.phg` — the full served app (W1 parse/serialize + W2 routing + the
  `respond` entry + `handle`); its `main()` exercises `respond` on canned `b"…"` requests so it stays
  byte-identical on `run`/`runvm` + real PHP. **Conformance** for the socket path lives in
  `tests/serve.rs` (an in-memory `FixtureTransport`, outside the byte-identity spine).

### M6 slice W1 — the HTTP handler model (`handle(Request) -> Response`, pure Phorge)

- **The portable handler contract** — `Request`/`Response` are ordinary Phorge classes and
  `parse_request(bytes) -> Request?` / `serialize_response(Response) -> bytes` are written in pure
  Phorge (PSR-7/15 shaped). Bodies are `bytes` (HTTP bodies are octets); the head is decoded ASCII for
  line/`:` splitting. Headers ride as `List<string>` raw lines with a `req.header(name) -> string?`
  linear-scan accessor (the method-call API is the public surface; a typed `Header` value arrives with
  S3). No socket yet — that is W3's `phg serve`. No new `Op`, no new `Value` variant.
- **`bytes.find(bytes, bytes) -> int?`** — first-occurrence byte search (`null` when absent, `0` for an
  empty needle, matching PHP 8 `strpos`); locates the CRLFCRLF head/body boundary. Erases to
  `(($p = strpos(…)) === false ? null : $p)`.
- **`text.split_once(string, string) -> List<string>`** — split on the first separator → `[head, tail]`
  (robustly parses `Name: value` headers whose value contains `:`). Erases to `explode($sep, $s, 2)`.
- **Example** `examples/web/handler.phg` — builds a canonical request as a `b"…"` literal, parses it,
  runs `handle`, and serializes the response (Content-Length recomputed from the body). Byte-identical
  on `run`/`runvm` + **real PHP**, auto-gated by the `examples/**/*.phg` glob.

### CLI binary renamed `phorge` → `phg`

- The CLI binary is now **`phg`** (matches the `.phg` extension; ripgrep's model — package `ripgrep`
  ships binary `rg`). All help/usage/version output, the cross-build `--bin`/artifact/cache names,
  release-asset naming, and docs use `phg`. The Cargo **package/lib name stays `phorge`**, as do
  `phorge.toml`/`phorge.lock`, the `.phorge` executable section, `PHORGE_*` env vars, and the
  `~/.cache/phorge` stub namespace.

### M6 slice W0 — the `bytes` type

- **`bytes`** — a new primitive: raw octet sequences distinct from UTF-8 `string`. `Value::Bytes`
  is `Rc`-shared (like `List`); `Ty::Bytes` is a built-in type name. No new `Op` — a `b"…"` literal
  rides the constant pool (`Op::Const`), interop rides `Op::CallNative`, `==` rides `Op::Eq`.
- **`b"…"` literals** — raw byte strings (no interpolation), escapes `\n \t \r \\ \"` plus `\xHH`
  (two hex digits → one arbitrary octet, so a literal can hold non-UTF-8 bytes).
- **`core.bytes`** interop module (`import core.bytes;`): `from_string(string) -> bytes`,
  `to_string(bytes) -> string?` (UTF-8 decode; `null` on invalid — composes with S2 `??`/if-let,
  never a fault), `len(bytes) -> int` (BYTE count, vs `core.text.len`'s character count),
  `concat(bytes, bytes) -> bytes`, `slice(bytes, int, int) -> bytes` (half-open, bounds-clamped —
  total, no fault).
- **Transpile** — `bytes` erases to PHP `string` (PHP strings are byte arrays); `b"…"` → a PHP
  double-quoted literal with `\xHH` preserved; the natives map to `strlen`/`mb_check_encoding`/`.`/
  `substr`. Example `examples/guide/bytes.phg` runs byte-identically on `run`/`runvm` + **real PHP**.
- First slice of the **M6 web-capabilities spike** (design-locked,
  `docs/specs/2026-06-18-m6-web-design.md`); bytes was pulled forward so HTTP bodies can be honest
  octets.

### M5 slice S3 — git dependencies + `phorge.lock` + `phg vendor` + auto-offline

- **`phg vendor`** — the only network-touching command. It clones each `[require]` git dependency
  at its pinned `tag`/`rev`, copies the dependency's source into `vendor/<vendor>/<package>/`, and
  writes `phorge.lock` pinning the **resolved commit SHA** + an FNV-1a-64 content hash. Idempotent and
  crash-safe (stages into a temp dir, swaps atomically, touches only each dependency's own subtree).
- **`phorge.lock`** (`src/lock.rs`) — a strict, deterministic TOML-subset lockfile (`[[package]]`
  blocks: `name`, `git`, `rev`, `hash`); round-trips through its own parser.
- **Auto-offline resolution** — `loader::load_project` merges vendored packages exactly like
  first-party library packages (mangle + resolve before any backend runs ⇒ `run` ≡ `runvm`
  structural; the transpiler de-mangles into `namespace …` blocks). `run`/`check`/`transpile`
  **never fetch** — they read the committed `vendor/`. New guards: `E-VENDOR-MISSING` (a `[require]`
  dep not vendored), `E-VENDOR-MAIN` (a vendored `package main`), `E-DUP-DEF` (a duplicate
  `(package, name)` after the merge — previously a silent overwrite).
- **Example** — `examples/project/withdeps/` (a project consuming a vendored `acme/strutil` library):
  ships its committed `vendor/` + `phorge.lock`; the project-aware differential harness loads it
  offline and gates `run` ≡ `runvm`, and it round-trips through real PHP. `phg vendor` gains a
  `--help` entry, USAGE/dispatch wiring, and three `phg explain` codes.
- **Tests** — `tests/vendor.rs` drives the real `git clone`/`checkout`/`rev-parse` path against a
  `file://` local-git fixture (offline, deterministic): fetch + lock + offline byte-identical load,
  idempotent re-vendor, and `E-VENDOR-MISSING`.

### M5 slice S2d — project-aware differential harness + public multi-file example

- **First public multi-file project** — `examples/project/tempconv/` (a two-package Celsius→Fahrenheit
  converter) showcases the M5 project model end-to-end: mandatory packages + folder=path, a
  cross-package qualified call (`convert.c_to_f(0)`), import aliasing (`import acme.label as fmt;` →
  `fmt.tag(...)`), and a same-package bare call across two files. Plus `examples/project/README.md`.
- **Project-aware byte-identity gate** — `tests/differential.rs` now discovers every project root (a
  directory with a `phorge.toml`) under `examples/`, loads it through `loader::load`, and asserts
  `run` ≡ `runvm` (and that it runs). The single-file glob is made project-aware — it stops descending
  into any directory holding a `phorge.toml`, so project files are never run standalone (structural,
  name-independent; flat examples keep their `len() >= 3` floor). A project added later is auto-gated.
- **Verified** — the example runs `freezing = 32F` / `boiling = 212F` byte-identically on `run`,
  `runvm`, **and real PHP 8.6** (exact integer math, chosen so PHP's float `/` agrees).
- Docs refreshed for shipped multi-file support: `examples/README.md` (index + matrix rows; the two
  "arrives in a later slice" notes corrected) and `FEATURES.md` (Modules/packages → 🚧, git deps = S3).

### M5 slice S2c — qualified cross-package calls + namespaced PHP + import aliasing

- **Cross-package calls resolve** — `import acme.util;` then `util.compute(x)` now works across files.
  A new resolution pass in the loader (`src/loader.rs`) mangles every non-`main` definition to a
  globally-unique name (`acme.util` + `compute` ⇒ `Acme\Util\compute`; `package main` defs stay bare),
  then rewrites call sites against each file's package + import map: same-package bare calls and
  qualified user calls become bare calls on the mangled name. Native `core.*` calls are untouched.
- **Import aliasing** — `import a.b as c;` binds the call-site leaf `c` (AST `Item::Import.alias`,
  parsed as a contextual `as` keyword so `as` stays a valid identifier). Resolves leaf collisions (O-9).
- **Namespaced PHP emission** (M5-7/M5-8) — a multi-package program transpiles to one
  `namespace Acme\Util { … }` brace-block per package + a `namespace Main { … }` block + a nameless
  `namespace { \Main\main(); }` bootstrap. Cross-package calls emit fully-qualified (`\Acme\Util\compute`);
  global-function natives gain a leading `\`. A single-package program has no mangled names and stays on
  the flat path — byte-identical to the pre-S2c output.
- **S2c scope: functions only** — a `class`/`enum` in a non-`main` (library) package is rejected
  (`E-PKG-TYPE`); cross-package type namespacing is an M5 follow-up. The S2b bare cross-package call
  interim is tightened: an unqualified cross-package call now fails on both backends.
- **Byte-identity** — resolution runs in the loader *before* any backend, so checker/interpreter/
  compiler/VM are unchanged (run==runvm is structural). Verified end-to-end: a two-file project runs
  `42` on `run`, `runvm`, **and real PHP 8.6** (`php out.php`).
- **`explain`** gains `E-PKG-TYPE` and `E-PKG-PATH` (the latter backfilled from S2b).
- 7 new tests (`tests/project.rs` qualified/alias/same-package-cross-file/unqualified-rejection/
  type-rejection/transpile-structure + a `native.rs` alias-`import_map` case). 409 total green.

### M5 slice S2b — multi-file loader + folder=path enforcement

- **Project loader** (`src/loader.rs`) — resolves an entry source to one `Unit` (a single, possibly
  multi-file-merged `Program` + the source text for diagnostics). **Project mode**: a `phorge.toml`
  found by walking up marks the root; every `.phg` under the source root is parsed, validated against
  its location (**folder = package**, Go's model — `src/acme/util/*.phg` ⇒ `package acme.util`;
  `package main` is folder-exempt), and all items are merged into one flat program. **Loose mode** (no
  manifest above): only `package main;` runs — a dotted library package requires a project.
- **`E-PKG-PATH`** — a file whose package does not match its directory under the source root, a dotted
  package sitting directly in the source root, or a non-`main` package living outside the source root.
- **Byte-identity preserved** — enforcement is path-aware and lives in the loader, never in the type
  checker, so `cli::cmd_run(&str)` and the differential harness are untouched. `run`/`runvm`/`check`/
  `transpile` route a `<file>` source through the loader (new `cli::run_program`/`runvm_program`/
  `check_program`/`transpile_program` consume the loaded program); `-e`, stdin, `parse`, `lex`,
  `disasm`, `bench`, and `build` keep the single-file string path. A loose single-file program through
  the loader produces identical output to the pre-S2b pipeline.
- **Flat-merge interim** — until S2c, the merged items share one flat namespace, so a cross-file call
  resolves **unqualified**; qualified cross-package calls (`util.parse(x)`) + one-brace-block-per-package
  PHP emission + import aliasing are S2c. `transpile` of a multi-*package* project therefore emits flat
  PHP for now (correct for `package main` / single-package). Multi-file type-error diagnostics omit the
  source-line caret (no single aligned source). The `examples/project/` showcase ships at S2d.
- 12 new tests (9 `loader` unit + 3 `tests/project.rs` integration, incl. a multi-file project running
  byte-identically on both backends).

### M5 slice S2a — project manifest + source root + project detection

- **`phorge.toml` manifest** — new `src/manifest.rs` parses a minimal, std-only TOML subset into
  `Manifest { name, version, source, require, require_dev }`. The manifest speaks **Composer's
  vocabulary in an honest TOML container**: `name = "vendor/package"` (doubles as the PSR-4 namespace
  root — `acme/myapp` ⇒ `Acme\Myapp`), `[require]` / `[require-dev]` sections, dependency values as
  `{ git = "…", tag|rev = "…" }` or the `"<git-url>@<tag>"` string shorthand. Each dep self-locates
  via its git URL (no Packagist, no Composer `repositories` side-table); versions are **exact-pin
  only** — a `branch` pin, a missing/double pin, an unknown key/section, or an unquoted value are hard
  errors. A literal `composer.json` was rejected on purpose: the `composer` tool cannot process it, so
  the filename would be a false promise.
- **Project detection** — `Project::detect(path)` walks up from a source file/dir for a `phorge.toml`;
  the first one found marks the project root and resolves the source root (`root/<source>`, default
  `src`). No manifest above ⇒ `Ok(None)` (loose-script mode). Manifest presence is the sole
  project-vs-loose signal (Go's model).
- **Byte-identity preserved** — S2a is parse + represent only; nothing consumes the manifest yet, so no
  `.phg` execution path changes and `run`/`runvm` stay byte-identical. The multi-file loader +
  folder=path enforcement (S2b), qualified cross-package calls + brace-namespace PHP (S2c), and the
  `examples/project/` showcase (S2d) follow. Coverage = 18 `manifest` unit tests (the showcase example
  ships with the observable behavior at S2d).

### M5 slice S1 — package declaration (project-model foundation)

- **Mandatory `package` declaration** — every file declares its package as the first line, never
  inferred (`package app.util;`). The reserved **`package main;`** is the runnable entry (Go's model;
  pairs with `fn main()`); `core` is reserved for the standard library. New checker codes
  `E-NO-PACKAGE` / `E-RESERVED-PACKAGE` (both `phg explain`-documented). The parser captures the
  path on `Program.package`; a `package` after any item is a parse error (it must be first).
- **Byte-identity preserved** — S1 is front-end only: the interpreter, VM, and transpiler ignore the
  package (flat PHP emission unchanged — `package main` → no namespace), so `run`/`runvm` and the PHP
  round-trip stay byte-identical. Multi-file projects, strict folder=path, cross-package imports, and
  brace-namespace PHP emission arrive in later M5 slices
  (`docs/specs/2026-06-18-m5-project-model-design.md`).
- All 24 examples + every test program migrated to `package main;`; the minimal program is now
  `package main;` + `import core.console;` + `console.println`. (Also fixed pre-existing Wave-1 doc
  drift: `README.md` showed `import std.io;` + bare `println`.)

### M3 slice S0 — developer experience

- **`var` local type inference** — `var x = expr;` infers the binding's type from its initializer
  (still fully static + immutable). The VM derives the local's operand type from the initializer, so
  arithmetic on a `var` still specializes (`AddI`/`AddF`); `ctype` now also resolves a `match` value.
- **`type` aliases** — `type Name = T;`, compile-time only. The checker resolves aliases (with cycle,
  built-in-shadow, and duplicate detection); a post-check pass (`checker::expand_aliases`) expands
  them out of the AST so the interpreter, VM, and transpiler all see alias-free types and the PHP
  output never mentions the alias.
- **Sharper diagnostics** — front-end (lex/parse/type) errors render the offending source line with a
  caret, attach a "did you mean `…`?" hint (nearest in-scope name, Levenshtein ≤ 2), and carry a
  stable code. `Diagnostic` gains `code`/`hint` fields + a `render` method; all construction is
  centralized through `Diagnostic::new`. Runtime-error strings are unchanged (differential parity).
- **`phg explain <CODE>`** — print the explanation for a diagnostic code (`E-UNKNOWN-IDENT`,
  `E-UNKNOWN-TYPE`, `E-INFER-NULL`, `E-ALIAS-CYCLE`).
- **Per-command help** — `phg <command> --help` / `-h` prints a description, the source/flag forms,
  and 1–2 worked examples.
- New guide example `examples/guide/inference.phg` (auto byte-identity-gated by the differential
  harness).

### M3 slice S1 — core ergonomics

- **List indexing `xs[i]`** — un-rejected in both backends (the checker already typed it), reusing the
  bounds-checked `Op::Index`. An out-of-range read is a clean `list index out of range` runtime fault,
  byte-identical across `run`/`runvm` (classified `FaultKind::IndexOob` in the differential harness).
  Transpiles to `$xs[$i]`.
- **Integer ranges `a..b` / `a..=b`** — exclusive / inclusive integer ranges, materialized to a
  `List<int>` by the one new `Op::MakeRange(bool)` (which extends the three coupled matches —
  `vm::exec_op`, `compiler::stack_effect`; `chunk::validate` needs no arm: no static index). Both
  backends build the list via Rust's native `start..end` / `start..=end` (no counter overflow), so
  `for (int i in 0..n)` works unchanged. The lexer adds `..` / `..=` (longest-match). Transpiles to PHP
  `range()`; a non-int bound is `E-RANGE-TYPE` (a `phg explain` entry).
- **Expression `if`** — `if (c) { e } else { e }` in value position (`var x = if (c) { 1 } else { 2 };`).
  Parens + a mandatory `else`; single-expression arms. Disambiguated from the statement `if` by parse
  position; lowers to the existing branch ops (no new `Op`); transpiles to a PHP ternary.
- New guide example `examples/guide/ergonomics.phg` (indexing + ranges + expression `if`),
  auto byte-identity-gated and round-tripped through real PHP.
- **S1.4 (smart-cast narrowing) deferred to S2** — it only narrows optionals (`T?`), which arrive in S2.

### M3 slice S2 — null-safety

PHP-native nullable with a compile-time non-null guarantee (TypeScript `strictNullChecks` over PHP's
nullable runtime). `T?` is the existing `null` value at runtime; the guarantee lives in the checker
(a non-optional `T` can never be `null`). All byte-identical on `run`/`runvm` and 1:1 to PHP.

- **Optionals `T?` + non-null discipline** — `Ty::Optional` + `Value::Null`; `T` auto-widens to `T?`,
  but a `T?` cannot flow into a non-optional `T` (`E-OPT-ASSIGN`), nor be used as an operand/receiver
  without unwrapping (`E-OPT-USE`).
- **`??` null-coalesce** — `a ?? b`; `?.` safe access — `opt?.member` / `opt?.method()` short-circuits
  a null receiver to `null` (PHP `?->`). Both lower to a null-test + branch, **no new `Op`**.
- **`if (var x = opt)`** — binds the non-null inner `T` (smart-cast S1.4) inside the then-block only;
  `E-IF-LET-TYPE` on a non-optional scrutinee. Transpiles to `if (($x = E) !== null) { … }`.
- **`opt!` checked force-unwrap** — `T?` → `T`, a clean `force-unwrap of null` fault on null (never a
  crash; `FaultKind::ForceUnwrap` parity). `E-OPT-UNWRAP` on a non-optional; the **`W-FORCE-UNWRAP`**
  lint flags every use. Transpiles to a once-per-file `__phorge_unwrap()` helper.
- **`match` over `T?`** — `match opt { null => …, v => … }` is exhaustive; the binding arm narrows
  `v` to the non-null inner after a `null` arm.
- **Warning channel (first lint)** — the checker now collects non-fatal warnings; `check()` returns
  them on success and the CLI renders them to stderr without gating the build.
- **No new `Op` variant** — `Op::MatchFail` was generalized to `Op::Fault(FaultMsg)` (single-sourced
  message), serving both match-exhaustiveness and `opt!`-on-null.
- New guide example `examples/guide/null-safety.phg`, auto byte-identity-gated + PHP round-tripped.

### M3 Track B Wave 1 — namespaced native foundation

- **Everything is namespaced — "nothing in the wind".** The free global `println` is retired. A
  program now `import core.console;` and calls `console.println(...)`. Stdlib modules are reserved
  under the `core.*` root; the root lives in the import and the leaf qualifies the call (Go's
  `import "fmt"` → `fmt.Println`). Explicit import is required even for the stdlib.
- **`native` registry** (`src/native.rs`) — each built-in single-sources its four facets in one
  entry keyed by `(module, name)`: checker signature (`params`/`ret`), a runtime `eval` shared
  verbatim by the interpreter *and* the VM (structural parity, like the value kernels), and a PHP
  emission mapping (`console.println` → `echo … . "\n"`). Built once via `OnceLock`.
- **`Op::Print` → `Op::CallNative(idx, argc)`** — the migrated former print op now indexes the
  registry and pushes the native's result (extends the three coupled `Op` matches + a `validate`
  bound on the native index). No separate `Const(Unit)`.
- **Import-driven resolution across all four backends** — a member call `console.println(x)` whose
  head is an imported module qualifier dispatches to the native: the interpreter and compiler resolve
  locals-first then by leaf (they track scope); the checker and transpiler use the import map.
- **Shadowing guard** — a value binding may not shadow an imported module qualifier (`E-SHADOW-IMPORT`),
  keeping the import-map-driven transpiler consistent with the locals-first run backends.
- Migrated every `println` call site — all examples, fixtures, and inline test programs — to
  `import core.console;` + `console.println`. The example differential test now also asserts each
  example *runs* (`Ok`), not merely that the backends agree (closing a vacuous-green gap).

### M3 Track B Wave 2 — stdlib breadth (`core.math` / `core.text` / `core.file`)

- **`core.math`** — `sqrt`/`pow`/`floor`/`ceil` (float) and `abs`/`min`/`max` (int). Concrete-typed
  (the registry's `params`/`ret` have no type variable, so no overloading); each erases to the PHP
  builtin of the same name. `abs` faults cleanly on `i64::MIN` (EV-7).
- **`core.text`** — `len`/`upper`/`lower`/`trim`/`contains`/`split`/`join`/`replace`. `split` returns
  `List<string>` and `join` consumes one (the type system already carries `List<string>` end to end).
  The PHP erasures reorder args where PHP differs (`explode`/`implode` separator-first, `str_replace`
  search-first).
- **`core.file`** — `read` (→ `string?`, `null` on any failure — composes with the S2 `??` / if-let),
  `exists`, and `write`. File *reads* stay byte-identical by reading a **committed fixture**
  (`examples/guide/fixtures/poem.txt`); `write` is a non-deterministic side effect, unit-tested but
  kept out of the byte-identity-gated example set.
- Each module ships a byte-identity-gated guide example (`examples/guide/math|text|file.phg`),
  round-tripped through real PHP. `KNOWN_ISSUES` now documents the pre-existing irrational-`float`
  precision divergence that `core.math` makes easy to reach (Rust shortest-round-trip vs PHP's
  default `echo` precision); examples keep to exactly-representable values.
- **Deferred:** `core.list` (needs S3 lambdas / `List<T>` generics) and `core.json` (needs a dynamic
  `Json` type) — they land once generics or S3 exist.

_Next: Track B Wave 3 (user packages: `package` decl + folder=path + PHP `namespace` emission), then
Track A (S3 lambdas/pipeline). M2.5 Phase 3 (CI stub registry; opt-in `--sign`) remains parked._

## [0.4.0] — 2026-06-17

The first fully-documented release: CLI UX, profiling, a disassembler, cross-OS standalone builds,
and a complete OSS doc set.

### Profiling & introspection

- `phg bench` now reports **memory** alongside timing: peak-RSS growth of one cold execution plus
  the process `VmHWM`/`VmRSS`, via a std-only, Linux-only `src/mem.rs` (`/proc/self/status` +
  `/proc/self/clear_refs`). Non-Linux hosts print `memory: unavailable on this platform`.
- `phg disasm <source>` — print the compiled bytecode: per-function instruction listings (index,
  source line, op, and a resolved annotation for index-carrying ops) plus the program-level
  enum/class/method descriptor tables.
- New profiling example `examples/bench/workload.phg` (CPU recursion + heap allocation) with
  `examples/bench/README.md` documenting how the time and memory numbers are collected.

### CLI UX

- `-v` / `--version` — print `phg <version>` and exit; `-h` / `--help` — full usage banner.
- Flexible program source for the run-family commands
  (`run`/`runvm`/`check`/`parse`/`lex`/`transpile`/`disasm`/`bench`): `<file>` | `-` (read from **stdin**) |
  `-e <code>` / `--eval <code>` (run **inline** source) | `--` (next arg is a path even if it starts
  with `-`).

### M2.5 Phase 2 — cross-OS standalone builds

- `phg build --target <triple>` / `--all` cross-compiles a runtime stub via
  [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild) (zig as the linker) and embeds the
  program as a named object-file section. Targets: `x86_64-unknown-linux-musl`,
  `aarch64-unknown-linux-{gnu,musl}`, `x86_64-pc-windows-gnu`.
- `src/bundle.rs` → a `bundle/` module: CRC-guarded `container`, per-format readers `elf`/`pe`/`macho`
  (thin + fat), a magic-sniffing `section::find_section` dispatcher, and a `cross` orchestrator. The
  hand-rolled, std-only **PE/COFF**, **Mach-O 64**, and **fat/universal** readers use checked arithmetic
  (EV-7: adversarial input → `None`, never a panic) so a produced binary self-reads its own format.
- Stub cache keyed on an FNV-1a-64 of the phg binary's own bytes (a rebuilt phorge invalidates stale
  stubs, protecting the parity spine). Precise "missing rustup target" / "needs a source checkout"
  errors. apple/darwin targets are rejected with a clear message (macOS stub deferred to Phase 3; the
  Mach-O reader ships and is tested). `--sign` reserved for Phase 3.
- Cross-parity tests (toolchain-gated): `x86_64-musl` native-execution parity vs `runvm`, and a real
  windows-PE section round-trip.

### Documentation

- Full OSS project doc set: rewritten README, dual **MIT OR Apache-2.0** license, CONTRIBUTING,
  CODE_OF_CONDUCT, SECURITY, SUPPORT, GOVERNANCE, AUTHORS, ROADMAP, VISION, FEATURES, KNOWN_ISSUES,
  THIRD-PARTY-NOTICES, CITATION.cff, `.editorconfig`, and `.github/` templates.

Built standalone binaries are unchanged: they run their embedded program and ignore argv.

## [0.3.0] — 2026-06-16

First tagged POC. Usable end-to-end on `x86_64-linux-gnu`: the full M1 language on two
byte-identical backends (`run` interpreter + `runvm` bytecode VM), a Phorge→PHP transpiler, and
`phg build` producing a standalone native Linux executable. Bundles all post-M2-P3 work — the
P3.5 hardening pass, M2 P4 (classes/enums/match/methods), Wave 4 (class-aware compiler types), P5a
(`Rc`-shared heap), the full-coverage example set, and M2.5 Phase 1 (standalone build). Known v1
limits: `build` is host-only; the artifact ignores argv and always exits 0; the language has no
indexing/`Map`/`Set`/optionals/`|>`/exceptions/mutation (all M3).

### M2.5 Phase 1 — `phg build` (x86_64-linux-gnu) (2026-06-16) — **distribution**
`phg build foo.phg` produces a standalone host executable that runs `foo.phg` on the VM with no
Phorge install — by copying the running phg binary, embedding the program **source** in a
`.phorge` ELF section, and self-detecting + running that payload at startup. Same section+container
mechanism as the cross-OS end state (design §7). See
`docs/specs/2026-06-16-m2.5-phorge-build-design.md` + `docs/plans/2026-06-16-m2.5-phase1-build-linux-gnu.md`.

- **Added**
  - `src/bundle.rs` (std-only, zero new deps): a bitwise CRC-32, a versioned CRC-guarded payload
    **container** (`magic | version | header_len | kind | comp | enc | flags | len | payload_crc32 |
    header_crc32`), a hand-rolled **ELF64 section reader** (no `object`/`goblin` — it links into the
    produced binary, so it must stay zero-dep), and `embedded_source()` (graceful `None` on every
    malformed/tampered/absent input).
  - `cli::cmd_build` — validates the program (no broken binary is ever emitted), copies `current_exe`,
    and shells `llvm-objcopy --add-section .phorge=…` (override via `PHORGE_OBJCOPY`).
  - `phg build <file> [-o out]` CLI command; `main()` runs an embedded payload at startup before
    any arg parsing.
  - `tests/build.rs` — the parity spine extended to distribution: a built binary's output is
    byte-identical to `runvm`; argv is ignored (v1); ill-typed programs fail with diagnostics and
    emit no binary.
  - **Hardening (post-review):** the ELF64 reader uses fully-checked offset arithmetic — adversarial/
    malformed input returns `None`, never overflow-panics under the debug/test profile
    (regression-tested per EV-7); `phg build` rejects a dangling `-o`, an unrecognized flag, or any
    extra argument with a usage error (exit 2) instead of a silent default-named build. `docs/INVARIANTS.md`
    #1 now records the build binary as the third `cmd_runvm` parity surface.
- **Notes** (v1 limits) — host-only (`x86_64-linux-gnu`); the embedded program ignores argv and
  cannot set a custom exit code; the source is recoverable from the artifact (not obfuscated).
  Cross-targets (zig), PE/Mach-O reader arms + stub cache = Phase 2; CI stub registry + signing/
  notarization (rcodesign-from-Linux) = Phase 3.

### Examples — full-coverage showcase (2026-06-16) — **docs/tests**
A living example set covering the entire runnable language surface, plus the Phorge→PHP bridge. See
`docs/specs/2026-06-16-examples-coverage-design.md` + `docs/plans/2026-06-16-examples-coverage.md`.

- **Added**
  - Four real-world programs (`examples/realworld/{ledger,library,shop,rpg}.phg`) and six focused
    guide programs (`examples/guide/{operators,control-flow,collections,classes,enums-match,strings}.phg`),
    each exercising a different slice of the surface; an `examples/README.md` index + coverage matrix.
  - `examples/transpile/{demo.phg,demo.php,README.md}` — the Phorge→PHP transpile bridge (the only
    PHP-ecosystem path: output, not input), with a `tests/cli.rs::transpile_demo_matches_committed_php`
    snapshot test that fails on transpiler drift.
- **Changed**
  - `tests/differential.rs` now **globs `examples/**/*.phg`** instead of listing examples explicitly,
    so every current and future example is byte-identity-gated with no test edit.
- **Notes** (honest boundary, documented in `examples/README.md`)
  - Zero-payload enum variants need call form `V()` to construct **and** in a `match` pattern — a
    bare `V =>` arm is a catch-all binding (a silent logic bug both backends agree on).
  - `import` is decorative (no module resolution until M5); `null`/`T?`/`Map`/`Set`/`|>`/exceptions
    /traits/overloading remain M3+ and are deliberately absent.

### M2 P5a — `Rc`-shared heap objects (2026-06-16) — **object-path perf**
Makes compound heap objects *shared* instead of *deep-cloned*. The M1 heap is immutable + acyclic
(no reassignment, no field mutation, args evaluated before the instance exists), so `Rc` is both
sufficient and complete for reclamation — `Drop` frees everything, no cycle can leak, no tracing
collector is needed (that stays deferred to M3). See
`docs/specs/2026-06-16-m2-p5-object-model-design.md` + `docs/plans/2026-06-16-m2-p5a-rc-shared-heap.md`.

- **Changed**
  - `Value::Instance(Rc<Instance>)`, `Value::Enum(Rc<EnumVal>)`, `Value::List(Rc<Vec<Value>>)`
    (were `Box`/`Vec`). Cloning a `Value` — the `Op::GetLocal` hot path and every interpreter
    var-read — is now an O(1) refcount bump instead of a deep `HashMap`/`Vec` copy. The constructor
    now shares one `Rc` between the `this` receiver and the returned instance (no double build).
  - Three move-out sites adjusted (can't move out of an `Rc`): `vm.rs` `GetEnumField`
    (`into_iter().nth` → `.get().cloned()`), the interpreter's list `for` (iterate by ref + clone),
    and the ctor double-build (folded into one shared `Rc`). No `Op`/bytecode/AST/checker change.
- **Perf** (`phg bench`, median of 101, `fib(28)`)
  - Object-heavy VM run **1537 ms → 634 ms (2.4× faster)**; the VM's advantage over the tree-walker
    recovered from **4.73× → 9.35×**, essentially on par with the scalar baseline (10.92×) — i.e.
    the object-path penalty (deep-clone-on-load) is largely eliminated.
  - **Phase B deferred (bench-gated, not opened):** slot-indexed `Vec` field layout. With the object
    path now ~within scalar's advantage, field access (HashMap lookup) is no longer dominating, so
    there is no evidence to justify the larger interpreter-touching change.
- **Parity** — behavior-preserving refactor; the full differential suite + examples sweep stay
  byte-identical (244 tests green), clippy + fmt clean, `#![forbid(unsafe_code)]` intact.

### M2 Wave 4 — Class-aware compiler types (2026-06-16) — **closes the last `num_ty` parity gap**
Makes the compiler's operand-type inference class-aware, so the VM no longer rejects checker-valid
programs that read a field of an arbitrary instance, a method-call result, or a nested member as an
arithmetic operand. `runvm` is now a faithful drop-in across the full checker-valid surface. See
`docs/plans/2026-06-16-m2-wave4-compiler-types.md`.

- **Changed**
  - The compiler's coarse `enum TyTag { Int, Float, Other }` became `enum CTy { Int, Float,
    Class(String), Other }` — an instance now carries *which class* it is, derived structurally from
    the AST's declared `Type` annotations (`type_tag` → `resolve_cty`); the AST, the `Op` set, the
    VM, and `value.rs` are untouched.
  - `num_ty` is now the numeric projection (`as_num`) of a new recursive `ctype(&Expr)` resolver
    that walks `Ident`/`This`/`Member`/`Call` to a class-aware type. New per-program tables —
    `class_field_ctys` (class → field → type) and `method_rets` (`(class, method)` → return type) —
    plus a `cur_class` on the compiler back the `Member`/method-call/`this` resolution. The
    P4c-era `this.field`-only `num_ty` `Member` arm is subsumed by the general resolver.
- **Parity**
  - Five programs that ran on the interpreter but failed to *compile* on the VM now agree
    byte-identically (`tests/differential.rs::WAVE4_PROGRAMS`): a field of an arbitrary instance
    (`p.x + 1`), a method result (`c.get() + 1`), a nested field (`a.inner.x + 1`), a class-typed
    enum payload bound in `match` (`Some(p) => p.x + 1`), and a free function returning an instance
    (`mk().x + 1`).
  - The only remaining coarse-type note is the deliberately out-of-M1-surface `Index` (`xs[i]`
    arithmetic faults on both backends — M1 has no user indexing).

### M2 P4c — Methods + `this` on the VM (2026-06-16) — **M2 P4 complete**
Brings instance methods and `this` to the bytecode VM. With this, **`runvm` covers the full M1
language surface** and `examples/grades.phg` runs on both backends. See
`docs/plans/2026-06-16-m2-p4-classes-enums-match.md`.

- **Added**
  - `Op::CallMethod(name_idx, argc)` — runtime method dispatch off the receiver instance's class,
    via a program-level `(class, method) → function index` table; the frame opens with the
    receiver at slot 0 (`this`).
  - Methods compile to functions (receiver at slot 0, params at `1..=argc`); `this` and bare field
    reads inside a method/ctor body resolve against the receiver.
  - `examples/grades.phg` joined the differential examples sweep; `phg bench examples/grades.phg`
    runs (VM ≈3.2× the tree-walker on it).
- **Removed**
  - The last two `(M2 P4)` compile-error stubs (`Expr::This`, method calls) — `grep "M2 P4"` in
    `compiler.rs`/`vm.rs` is now clean.
- **Parity notes**
  - Method existence is checker-enforced, so the VM's method-not-found fault is a defensive
    backstop (no `agree_err` case, like P4a's exhaustiveness).
  - `num_ty` now classifies a `this.field`/bare-field arithmetic operand (via the class's field
    tags). At this commit a field read on an *arbitrary* instance was still the coarse-`TyTag` gap;
    **closed in M2 Wave 4** (see the Wave 4 entry above) by making the type class-aware (`CTy`).

### M2 P4b — Classes on the VM (2026-06-16)
Brings class construction (with constructor promotion + body side effects) and field reads to the
bytecode VM. See `docs/plans/2026-06-16-m2-p4-classes-enums-match.md`.

- **Added**
  - `Op::MakeInstance` (build a `Value::Instance` from promoted-field values) and `Op::GetField`
    (runtime field lookup, with a `no field` fault byte-identical to the interpreter).
  - A program-level `ClassDesc` table (per-class promoted-field names) and an interned
    field-name pool, both validated by `BytecodeProgram::validate`.
  - Each constructor compiles to a synthetic `<Class>::new` function: it promotes its params into
    fields via `MakeInstance`, runs the body for side effects with the instance in scope, and
    returns the instance. `ClassName(args)` resolves to a `Call` into it.
- **Object model**
  - Instances are value-native: the VM reuses the shared `Value::Instance`, clone-on-use,
    mirroring the interpreter (decision P4-1). No arena.
- **Parity notes**
  - A ctor body's `return` is discarded and the promoted instance is always returned (interpreter
    parity): the synthetic ctor redirects body `return`s to an epilogue that loads + returns the
    instance, so an early `return;` cannot change the result.
  - Reading an explicit (uninitialized) `Field` member type-checks but faults `no field` at
    runtime on **both** backends — construction populates only promoted ctor params.
- **Known limitation at this commit (coarse-type gap — since closed in M2 Wave 4)**
  - A field read used as the *direct left operand* of arithmetic (`p.x + …`) couldn't be classified
    by the compiler's coarse `TyTag`. Field reads worked everywhere else: interpolation, equality,
    call arguments, arithmetic right-operand, or bound through a typed local first. **M2 Wave 4
    closed this** by making the compiler's type class-aware (`CTy`); see the Wave 4 entry above.
  - `examples/grades.phg` still needs P4c (it calls an instance method).

### M2 P4a — Enums + `match` on the VM (2026-06-16)
Brings single-payload enums and exhaustive `match` to the bytecode VM (already in the
interpreter since M1). See `docs/plans/2026-06-16-m2-p4-classes-enums-match.md`.

- **Added**
  - `Op::MakeEnum`/`MatchTag`/`GetEnumField` (enum construction, variant tag test, payload
    extraction) + `Op::MatchFail` (checker-unreachable non-exhaustive backstop, byte-identical
    to the interpreter's fault).
  - A program-level `EnumDesc` table (the enum analogue of the constant pool), validated by
    `BytecodeProgram::validate`.
  - Compiler operand-height tracking, so a `match` used mid-expression (e.g. as a binary
    operand, or nested in another arm) spills its scrutinee to the correct stack slot.
- **Object model**
  - Enums are value-native: the VM reuses the shared `Value::Enum`, clone-on-use, mirroring the
    interpreter (decision P4-1). No arena — deferred to a bench-gated perf milestone.
- **Known limitation (pre-existing, shared by both backends)**
  - `match` cannot appear inside string interpolation — the lexer's `{…}` interpolation does not
    nest a `match`'s braces. Not a parity issue (both backends reject it identically).

### M2 P3.5 — Hardening (in progress, 2026-06-16)
Closing the parity/no-crash contract gaps before P4 widens the surface. See
`docs/plans/2026-06-16-m2-p3.5-hardening-roadmap.md`.

- **Added**
  - `phg bench <file>` — median-of-N timing of both backends, output-identity gated; measures
    the "VM faster than tree-walker" thesis (≈10× on `examples/fib.phg`) instead of asserting it.
  - `agree_err` error-parity oracle in the differential harness (faults classified by semantic
    `FaultKind`).
  - Central `src/limits.rs` (recursion/nesting caps + numeric-width policy); unified
    `diagnostic::Diagnostic` for all stages; `BytecodeProgram::validate`; `docs/INVARIANTS.md`,
    `docs/ARCHITECTURE.md`; `rust-toolchain.toml`.
- **Changed**
  - Arithmetic/comparison single-sourced into `value.rs` (both backends call the same kernels).
  - VM runtime errors now carry the source line (`Chunk.lines`).
  - Constant pool interns scalar duplicates.
  - `interpreter::Frame` → `CallScopes` (removes the name collision with `vm::Frame`); scope-verbs
    unified (`push_scope`/`pop_scope`).
  - Quality gate is now compile-time (`warnings = "deny"`, `clippy.all = "deny"`,
    `#![forbid(unsafe_code)]`) + a tracked pre-commit hook.
- **Fixed**
  - `Op::Neg` on `i64::MIN` aborted the VM (P0) — now a clean `integer overflow` fault, matching
    the interpreter.
  - Interpreter/parser/checker no longer SIGABRT on deep recursion/nesting — explicit limits fault
    cleanly.
  - Determinism: checker's non-exhaustive-`match` error sorts its missing-variant list.

## M2 — Bytecode + VM (P1–P3, 2026-06-16)
- **P1** — `Chunk` + typed `Op` enum + stack VM dispatch loop.
- **P2** — AST→bytecode compiler for the `main`-only surface + `phg runvm` + the differential
  harness (`runvm` byte-identical to `run`).
- **P3** — user function calls, clox-style call frames, recursion/mutual recursion; `examples/fib.phg`
  runs on the VM.

## M1 — Tree-walking interpreter + transpiler — 2026-06-15 (`9da6e56`)
- Full pipeline: lexer → parser → type-checker → tree-walking evaluator.
- Phorge → PHP transpiler, round-trip-verified against real PHP.
- CLI: `phg <run|check|parse|lex|transpile>`.
- Language surface: static types, immutable-by-default bindings, functions, classes + constructor
  promotion, single-payload enums + exhaustive `match`, string interpolation, `List<T>` literals,
  `for…in`, checked int/float arithmetic. 162 tests green at the tag.
