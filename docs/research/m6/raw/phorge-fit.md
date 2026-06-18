# Phorge fit-analysis for an M6 web/HTTP capability

> How a web feature slots into Phorge's existing architecture, with the determinism quarantine
> (byte-identical `run`≡`runvm` spine) and the transpile contract as the dominating constraints.
> Every claim cites `file:line`. Read with `docs/INVARIANTS.md`, the M6 research plan
> (`docs/plans/2026-06-18-m6-web-capabilities-research.md`), and memory
> `m6-web-capabilities-direction`.

---

## 1. The byte-identity harness (`tests/differential.rs`) — where the spine is, and what sits outside it

### How examples are discovered + gated
- The spine is the pair `agree` / `agree_err`: `agree` runs `cmd_run` (interpreter) and `cmd_runvm`
  (VM) on the same source and `assert_eq!`s their `Result` structurally
  (`tests/differential.rs:28-36`); `agree_err` classifies *failures* by semantic `FaultKind`
  (body substring, prefix-independent) so both backends must fault the *same way*
  (`tests/differential.rs:71-109`). Both prepend `package main;` via `with_pkg`
  (`tests/differential.rs:20-26`) — M5 made every file packaged.
- **Single-file examples** are glob-discovered, not listed: `collect_phg` walks `examples/`
  recursively (`tests/differential.rs:539-551`) and `all_examples_match_between_backends`
  asserts each one **runs `Ok`** *and* `agree`s (`tests/differential.rs:597-622`). The explicit
  `cmd_run(&src).is_ok()` assertion (`:614-619`) is load-bearing: `agree` alone is *vacuously
  green* when both backends fail identically (e.g. a broken import), so the harness asserts success
  to catch a malformed example. **A new `.phg` is auto-gated the moment it lands — no test edit.**
- **Structural project exclusion**: `collect_phg` returns early if a dir holds a `phorge.toml`
  (`tests/differential.rs:540-542`) — a multi-file M5 project can't run file-standalone, so it's
  excluded by manifest presence (not by name). A project added later is auto-excluded.
- **Multi-file projects** are gated separately by `all_example_projects_match_between_backends`
  (`tests/differential.rs:631-659`): `collect_projects` finds every `phorge.toml` root
  (`:554-565`), `find_main_phg` locates the `package main` entry (`:569-592`), then it loads via
  `loader::load` and gates `run_program` ≡ `runvm_program` (`:644-657`), again with an explicit
  `run.is_ok()` (`:647-651`).

### The quarantine precedent — `tests/build.rs` and `tests/vendor.rs` live OUTSIDE the spine
- `tests/build.rs` gates `phg build`'s output against `phg runvm` (parity extended to the
  *distribution* layer, `tests/build.rs:1-3`) but is **a separate integration test file** — it is
  *not* part of the `agree`/glob spine. It is **toolchain-gated with graceful skip**:
  `cross_toolchain_ready` returns false (and the test no-ops) when cargo-zigbuild or the rustup
  target is absent (`tests/build.rs:8-24`). This is exactly how a non-deterministic / environment-
  dependent feature is tested without polluting the byte-identity spine.
- `tests/vendor.rs` is **the one network-touching code path, exercised offline**: its header
  states a live remote would be non-deterministic "the same reason URL/network features are
  deferred to M6", so it builds a throwaway local git repo and fetches it via a `file://` URL
  (`tests/vendor.rs:1-8`). Deterministic fixture substituting for a non-deterministic source.

**Read-through for the web server:** the socket accept-loop is *maximally* non-deterministic. It
belongs in a dedicated `tests/serve.rs` (like `build.rs`/`vendor.rs`), **never** in
`differential.rs`. The pure parts (bytes→Request parsing, routing, handler dispatch) belong in the
spine — they are deterministic functions of their input, exactly like every existing example.

---

## 2. The native registry (`src/native.rs`) — can `core.http` be pure natives?

### How a `(module, name)` native is registered
`NativeFn` single-sources all four facets so the four backends can't drift (`src/native.rs:21-38`):
`module` + `name` (the address), `params` + `ret` (the checker's signature), `eval:
fn(&[Value], &mut String) -> Result<Value, String>` (runtime behavior **shared verbatim** by
interpreter and VM — the structural-parity guarantee), and `php: fn(&[String]) -> String` (the
transpile-time PHP erasure). The table is built once in `build()` (`:421-446`), pinned at slot 0 =
`CONSOLE_PRINTLN` (`:43`, self-checked at `:440-444`), behind a `OnceLock` (`:450-453`). Lookups:
`index_of(module, name)` for the checker/transpiler (carry the import map) and
`index_of_by_leaf(leaf, name)` for the interpreter/compiler (locals-first scope tracking)
(`:457-473`). `import_map` binds leaf qualifier → dotted path, honoring `as` aliases (`:479-492`).

### `core.file` is the closest precedent — real I/O that stays deterministic
- `core.file` does **real filesystem I/O**: `file_read` calls `std::fs::read_to_string`
  (`src/native.rs:355-364`), `file_exists` calls `Path::exists` (`:365-370`), `file_write` calls
  `std::fs::write` (`:371-379`). So **a native already performs side-effecting OS calls today** —
  the registry is not restricted to pure functions.
- **How it stays deterministic** (header comment `:347-353`): a file *read* is byte-identical
  across backends *iff every backend reads the same bytes*, so file **examples read a committed
  fixture** (`examples/guide/fixtures/`). `write` is a non-deterministic side effect and is
  **excluded from the byte-identity-gated example set** — it is unit-tested against a temp file
  instead (`file_natives_eval_and_emit`, `:677-692`). Crucially, the run↔runvm spine shares the
  same `eval`, **so it is always identical regardless** — the determinism risk is only against
  *PHP* (the transpile round-trip) and against *re-runs*, not between the two Rust backends.
- `file_read` returns `string?` — any failure maps to `Value::Null`, never a fault (`:355-364`),
  composing with S2 null-safety (`??`, `if (var x = read(p))`).
- **PHP mapping** of `core.file`: `read` → `(($__c = @file_get_contents({})) === false ? null : $__c)`
  (the `@`+compare distinguishes missing from empty so it matches the `string?` semantics, `:392-397`);
  `exists` → `file_exists(...)` (`:405`); `write` → `file_put_contents(...)` (`:413`). The namespace
  is a compile-time organizing layer — a `core.*` native erases to a flat PHP builtin (decision N-2,
  module doc `:6-8`).

### Could `core.http` be hosted as natives, or does it need language types?
**Mixed — and this is the central design decision.** Two sub-questions:

1. **Request/Response value types.** Phorge's value model already has `Value::Instance(Rc<Instance>)`
   (`src/value.rs:27`, an `Instance` = class name + `HashMap<String,Value>` fields, `:31-35`) and the
   type system has `Ty::Named(String)` for nominal class/enum types (`src/types.rs:13`). It also has
   `Value::Map` / `Ty::Map(K,V)` (`src/value.rs:25`, `src/types.rs:15`) for headers, and `Value::Null`
   / `Ty::Optional` (`src/value.rs:21`, `src/types.rs:18-19`) for optional fields. **So Request and
   Response are most naturally `class`es written in Phorge itself** (a `package core.http` library, or
   stdlib-provided classes) — not native Rust types. A handler is then an ordinary
   `function handle(Request) -> Response` — already fully supported by both backends (P4b/P4c classes
   + methods are in the spine, `tests/differential.rs:438-531`). **No new `Value` variant, no new
   `Ty`.** The blocker noted in the M5 prose — that library packages "export functions only"
   (`E-PKG-TYPE`, `src/cli.rs:194-199`) — means cross-package *types* aren't done yet; until that
   follow-up lands, the http types live in `package main` or are stdlib-builtin classes.
2. **Construction/parsing/accessor natives.** `parse_request(bytes) -> Request?`,
   `response_text(status, body) -> Response`, header getters, etc. *could* be natives — but a native's
   `eval` returns a `Value`, and constructing a `Value::Instance` of a user class from Rust is awkward
   (the native would need the class name + field layout). **Cleaner: write the parser/builders in
   Phorge** (pure Phorge functions over `string`/`List`/`Map`), so they are spine-tested like any
   example and transpile for free. Reserve natives for the genuinely-primitive ops Phorge can't
   express: the raw socket read/write (which is the *dirty* layer, see §6) and possibly a fast
   byte-level split. **Recommendation: `core.http` parsing + Request/Response are Phorge code; the
   only native is the transport (`core.net`/`serve` runtime), which is dirty and lives outside the
   spine.**

A KNOWN_ISSUES-style caveat inherited from `core.text`/`core.file`: `Value::Str` is UTF-8
(`src/value.rs:17`) while HTTP bodies are octets — the M6 research plan already flags a possible
`bytes` type (memory `m6-web-capabilities-direction:30`). String-bodied examples must stay ASCII to
round-trip through PHP (same constraint as `core.text`, `src/native.rs:187-191`).

---

## 3. The Op set + CallNative path — does a web feature need a new `Op`?

**No new `Op` is required.** The chain of evidence:
- The invariant: adding an `Op` variant requires extending three exhaustive matches *in one commit* —
  `chunk.rs` `validate`, `compiler.rs` `stack_effect`, `vm.rs` `exec_op` (memory
  `op-variant-match-coupling`; `chunk.rs:262-270`).
- `Op::CallNative(idx, argc)` is already the namespaced stdlib's **single runtime entry point**
  (`src/chunk.rs:118-122`). It is fully generic: multi-arg, typed, value-returning. `console.println`,
  all of `core.math`/`core.text`/`core.file` already flow through it.
  - `validate` bounds the native index against `registry().len()` (`src/chunk.rs:303-305`,
    `:282`).
  - `stack_effect` is `1 - argc` — pops args, pushes one result (`src/compiler.rs:591-593`).
  - `exec_op` pops `argc` args, runs the shared `eval` (threading `self.out`), pushes the result
    (`src/vm.rs:266-273`).
- **So every new native is purely additive**: append a `NativeFn` to a `*_natives()` builder, and all
  four backends pick it up with zero new ops, zero plumbing. The `core.file` wave confirms it: "the
  four-backend call path was already fully generic … each module was purely additive — no plumbing
  changes" (project CLAUDE.md, Track B Wave 2).
- **Precedent that even a new *fault* needs no new op**: S2 generalized `Op::MatchFail` →
  `Op::Fault(FaultMsg)` (`src/chunk.rs:141-147`, `:44-61`) so `opt!`-on-null reused one op. If the
  web layer needs a fixed runtime fault, add a `FaultMsg` variant — still no new `Op`.

**Conclusion:** the web handler model is pure natives + Phorge code over the existing op set. The only
thing that can't be an op is the socket accept-loop — and that doesn't run in the VM at all (§6).

---

## 4. The transpile target (`src/transpile.rs`) — `handler(Request)->Response` → stock PHP; `phg serve` → `php -S`

### How a handler transpiles to idiomatic PHP
- Classes already transpile: `emit_class` produces a PHP `class` with constructor promotion
  (`src/transpile.rs:322-400`); methods via `emit_function(_, is_method=true)` (`:393`, `:265-296`);
  field reads resolve to `$this->field` (`resolve_ident`, `:791-803`). So a `Request`/`Response`
  class and a `handle(Request r) -> Response` function transpile with **no new transpiler code**.
- Native calls erase via the `php` closure resolved through the import map
  (`emit_member_call`, `:645-682`) — so an `http` native erases to its PHP form, and a global-function
  erasure gets a leading `\` inside a namespace block (`looks_like_global_call`, `:63-78`, `:668-672`).
- **The idiomatic stock-PHP target for a web handler** is: superglobals in (`$_SERVER`, `$_GET`,
  `$_POST`, `php://input`) → build the `Request` instance → call `handle($req)` → `echo` the response
  body + `header()` the status/headers. This is the PHP-CGI/`php -S` request model. The transpiler's
  job for `core.http` natives is to map Request *accessors* to superglobal reads
  (e.g. `req.path()` → `$_SERVER['REQUEST_URI']`) and Response *emission* to `header()` + `echo`.
  This mirrors `console.println` → `echo … . "\n"` exactly (`src/native.rs:428-433`).

### Where the `phg serve` → `php -S` seam is
- **There is no `serve` in the transpiler** — and there shouldn't be. The transpiler emits the
  *handler script*; `php -S` is the *server* that invokes that script per request. The seam is the
  **CLI**, not the language: `phg serve app.phg` runs Phorge's own socket loop calling the Phorge
  handler; `php -S localhost:8000 app.php` (the transpiled output) is the PHP-side equivalent. They
  are two runtimes wrapping the **same pure handler** — precisely the §6 three-layer split.
- Concretely: `transpile` already exists as a Program-taking runner (`cli::transpile_program`,
  `src/cli.rs:424-429`) and a string runner (`cmd_transpile`, `:469-474`). The web transpile reuses
  them unchanged; only the `core.http` natives' `php` closures are new. `phg serve` is a **new CLI
  command** (§5), parallel to `build`/`vendor`, that imports the *runtime*, not the transpiler.

---

## 5. CLI structure (`src/cli.rs` + `src/main.rs`) — where `phg serve` slots in

### How commands dispatch
- `main.rs` is a thin dispatcher. The command whitelist is a single `match` on `args[1]`
  (`src/main.rs:42-51`); each command is `fn(&str)->Result<String,String>` in `cli.rs`
  (`src/cli.rs:1-4`). Run-family (`run`/`runvm`/`check`/`transpile`) is **project-aware** — routed
  through `loader::load` for a `<file>` (multi-file merge + folder=path), loose for `-e`/stdin
  (`src/main.rs:180-203`). `parse`/`lex`/`disasm`/`bench` keep the single-file string path (`:204-223`).
- **The dirty-work precedents are `build` and `vendor`** — both bypass the run-family source-resolution
  path entirely:
  - `vendor` resolves a *project* (not a program), is the **only network-touching command**, has its
    own dispatch block (`src/main.rs:81-97`), its own `cmd_vendor` (`src/cli.rs:275-284`), and its own
    `tests/vendor.rs`. **This is the template for `serve`.**
  - `build` has its own arg-parsing block for `-o`/`--target`/`--all` (`src/main.rs:101-167`),
    `cmd_build` validates via `cmd_check` first (never emits a broken artifact, `src/cli.rs:435-448`),
    then delegates to `bundle::cross`. Its parity is gated by `tests/build.rs`, not `differential.rs`.

### Where `serve` slots in
`phg serve <file> [--port N]` is a **new tooling command modeled on `vendor`/`build`**:
- Add `"serve"` to the whitelist (`src/main.rs:42-46`) and the `USAGE` string (`:9-11`); add a
  per-command help arm in `help_for` (`src/cli.rs:55-137`).
- Add a dedicated dispatch block (like `vendor`'s, `src/main.rs:81-97`) that parses `--port`, loads the
  program (reusing `loader::load` so a multi-file project's handler works), validates it via the gate,
  then enters the **socket accept-loop in a new `src/serve.rs` module** (the dirty layer, §6).
- `serve` does **not** return text-to-print like the other commands — it blocks. So its dispatch
  arm prints a startup line and runs until interrupted, rather than going through the
  `Ok(text)=>print!` tail (`src/main.rs:224-230`).
- **`#![forbid(unsafe_code)]` (`src/main.rs:3`) and std-only** stand: `std::net::TcpListener` is safe
  std — no crate, no `unsafe`. **HTTP-only, no TLS** (TLS needs a crypto crate → breaks zero-dep +
  forbid-unsafe; it's the reverse-proxy's job, like `php -S`) — locked in memory
  `m6-web-capabilities-direction:28-29`.

---

## 6. The `Transport` seam — drawing the pure/dirty line

### The three layers (the key insight, from memory `m6-web-capabilities-direction:23-26`)
1. **Pure (in the spine):** `bytes → Request` parsing, routing, and `handle(Request) -> Response`
   dispatch. These are deterministic functions of their input — testable with `agree`/`agree_err`
   exactly like every existing example, and transpilable to PHP. Written as Phorge code +/- a small
   parsing native.
2. **Dirty (outside the spine):** the `TcpListener::accept()` loop — read raw bytes off the socket,
   hand them to layer 1, write `Response` bytes back. Maximally non-deterministic (timing, client
   identity, concurrency). Lives in `src/serve.rs`, driven by `phg serve`, tested by a thin
   `tests/serve.rs`.
3. **Transpile:** the same layer-1 handler → PHP superglobals+echo; `php -S` is the layer-2 equivalent
   on the PHP side (§4).

### The minimal trait/function seam
The pattern to copy is **the env-update HTTP-fixture seam** (`_GS_EU2_HTTP_FIXTURE_DIR` in `/stack`)
and Phorge's own `vendor.rs` `file://` fixture: abstract the byte source so a deterministic fixture
can substitute for a live socket. Minimal Rust seam:

```rust
// src/serve.rs (dirty layer — outside differential.rs)
pub trait Transport {
    fn next_request(&mut self) -> std::io::Result<Option<Vec<u8>>>; // raw request bytes, None = closed
    fn respond(&mut self, bytes: &[u8]) -> std::io::Result<()>;     // raw response bytes
}

// Real impl: wraps std::net::TcpListener / TcpStream (the only non-deterministic code).
// Test impl: a Vec<Vec<u8>> of canned requests + a captured Vec<u8> of responses (deterministic).

// The PURE core — fully unit/fixture-testable, no socket:
pub fn handle_raw(req_bytes: &[u8], dispatch: &PhorgeHandler) -> Vec<u8> { /* parse → route → run → serialize */ }
```

- `handle_raw` (bytes→bytes through the Phorge handler) is **deterministic** → unit-tested with a
  fixture `Transport`, and its Phorge-level half (`parse_request`/`handle`/`serialize`) lives in the
  byte-identity spine via an `examples/` program (below).
- The real `Transport` (TcpListener) is the *only* non-deterministic code — wrapped behind the trait,
  exercised by **one thin `tests/serve.rs`** that binds an ephemeral port, sends one real request, and
  asserts the response (toolchain/skip-aware like `tests/build.rs:8-24` if a port can't be bound).
- This is the same boundary `bench --vs-php` already respects: it spawns a real `php` process
  (`src/cli.rs:604-684`) but **output-identity-gates** the result, treating any divergence as a
  transpile bug, not a timing result (`:639-646`).

---

## Examples-ship-with-features mandate

The developer rule (project CLAUDE.md; memory `examples-ship-with-features`): every shipped feature
lands with a runnable, byte-identity-gated example *in the same change*, and CLI/tooling features that
aren't a single program get a walkthrough README + a small companion `.phg`.

**The web example is therefore two-part — exactly mirroring `examples/build/` and `examples/cli/`:**
1. **A pure handler example** under `examples/` (e.g. `examples/web/handler.phg` or a
   `examples/project/webapp/` project): defines `Request`/`Response` (classes), a
   `parse_request(string) -> Request?` and `handle(Request) -> Response`, and a `main()` that feeds a
   **committed fixture request string** through `handle` and `console.println`s the serialized
   response. This is **deterministic** → auto-gated by the `examples/**/*.phg` glob
   (`tests/differential.rs:597-622`) the moment it lands, runs byte-identically on both backends, and
   round-trips through real PHP. (Fixture-driven, like `examples/guide/file.phg` reading
   `examples/guide/fixtures/`, `src/native.rs:347-353`.)
2. **A README walkthrough for the live server** under `examples/web/README.md` (cf.
   `examples/build/README.md` 2.1K, `examples/cli/README.md` 3.4K) showing `phg serve app.phg`,
   the `curl` against it, and the `phg transpile app.phg | php -S` equivalent. The live socket
   loop **cannot be a runnable byte-identical example** (non-deterministic — same reason a fault can't
   be one, project CLAUDE.md), so it is captured in the README + the thin `tests/serve.rs`, not the
   glob.
3. Add an `examples/README.md` index + coverage-matrix row (the living surface, `examples/README.md`).

---

## Summary of slot-in points (file:line)

| Layer | Where it goes | Spine status |
|---|---|---|
| Request/Response | Phorge `class`es (`Value::Instance` `value.rs:27`, `Ty::Named` `types.rs:13`); `Map` for headers (`value.rs:25`) | in spine (P4b/P4c classes already gated) |
| Parsing / routing / dispatch | Phorge functions + (optional) parsing native via `Op::CallNative` (`chunk.rs:118-122`, `vm.rs:266-273`) — **no new Op** | in spine (deterministic) |
| `core.http` natives | append `NativeFn`s to a `http_natives()` builder in `native.rs` (purely additive, `:421-446`) | `eval` in spine; `php` erasure to superglobals+echo |
| Socket accept-loop | new `src/serve.rs` behind a `Transport` trait (`std::net`, safe, std-only) | **outside spine** (`tests/serve.rs`, skip-aware) |
| `phg serve` CLI | new command modeled on `vendor` (`main.rs:81-97`) / `build` (`main.rs:101-167`) + `cli.rs` help arm | tooling, not language |
| Transpile | reuse `transpile_program`/`emit` unchanged; only new `core.http` `php` closures | round-trip-gated like `bench --vs-php` |
| Examples | pure handler `.phg` (glob-gated) + `examples/web/README.md` (live server) | matches build/cli precedent |
