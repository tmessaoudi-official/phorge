# Phorge Playground (WASM) — Design

**Date:** 2026-06-24
**Status:** Approved (brainstorming gate passed) — pending spec review → implementation plan
**Topic:** A free, zero-backend, browser-based Phorge playground that runs all three backends live and
auto-deploys the latest `phg` on every `master` push.

## Goal

A single static web page where a visitor edits Phorge source and immediately sees:

- `run` (tree-walking interpreter) output,
- `runvm` (bytecode VM) output,
- the **transpiled PHP source** *and* its **executed** output (via php-wasm, PHP 8.4), and
- live type-check diagnostics with clickable `phg explain` codes,

with a badge confirming the three backends produced **byte-identical** stdout (and a diff banner when
they do not — a live correctness showcase). The deployed playground is rebuilt from the latest library
on every push to `master`, so "always the latest `phg`" is automatic.

This is an **adoption / visibility** lever, not a GA-gating milestone. It is already a roadmap line item
(`phg fmt`/repl/LSP/**playground** under M12/M7 DX).

## Why WASM, client-side, no backend

The language implementation is **zero external crates** (`[dependencies]` is empty), `#![forbid(unsafe_code)]`,
std-only, with a clean pure-computation public lib surface (`lexer`/`parser`/`checker`/`interpreter`/
`vm`/`compiler`/`transpile`). The host-only modules (`bundle`, `bench`, `cli`, `vendor`, `mem`/`/proc`,
`main`) — the only ones that touch `std::process`/`std::thread`/`/proc` — are never called by a playground.

This makes `wasm32-unknown-unknown` a near-perfect target:

- **Free hosting** — a static bundle on GitHub Pages; $0, no servers.
- **No sandboxing burden** — the browser is the sandbox; we run arbitrary user code with zero server risk.
- **Tiny blob** — zero-dep core ⇒ small wasm.
- **Trivial "always latest"** — CI recompiles the lib to wasm on every push; the page *is* the lib.

A server-side `phg run` service was rejected: it needs process isolation, timeouts, and resource caps
for arbitrary code, free tiers sleep/cost money, and it discards Phorge's portability advantage.

## The php-wasm linchpin (3-way execution)

`seanmorris/php-wasm` (MIT, the most active in-browser PHP project) ships a browser `php-wasm` package
that **defaults to PHP 8.4** — an exact match to Phorge's transpile floor — and runtime-loads extensions
only on demand. Phorge's transpile is **tier-1 only** (runs under `php -n`, no ini extensions), so the
default no-extension php-wasm environment matches the oracle's environment. It is CDN-distributable
(jsDelivr), so a fully static site can load it at runtime.

This is what makes "full 3-way from day one" viable: we execute the transpiled PHP *in the browser* and
compare its stdout to the two Rust backends.

## Crate layout — workspace, isolated playground crate

Convert the single crate into a Cargo **workspace**; the root `phorge` crate (lib + `phg` bin) is
**unchanged** and stays zero-dependency / `forbid(unsafe)`. A new `playground/` member is the only place
`wasm-bindgen` appears.

```
phorge/
├── Cargo.toml                       # + [workspace] members = ["playground"] ; root package unchanged
├── src/…                            # core — untouched, zero-dep, #![forbid(unsafe_code)]
├── playground/
│   ├── Cargo.toml                   # [lib] crate-type=["cdylib"]; deps: phorge { path = ".." } + wasm-bindgen
│   ├── src/lib.rs                   # #[wasm_bindgen] exports → JSON strings
│   └── web/
│       ├── index.html
│       ├── main.js                  # CodeMirror 6 + php-wasm glue + tabs + permalink + examples
│       ├── style.css
│       └── examples.js              # GENERATED at build time from examples/guide/*.phg
└── .github/workflows/playground.yml # build wasm + assemble dist/ + deploy to GitHub Pages
```

The `forbid(unsafe_code)` lint is per-crate; the `playground` crate may rely on wasm-bindgen's generated
code without affecting the core crate's invariant. The core crate's existing tests, `phg` bin, and
`differential.rs` are unaffected by the workspace conversion.

## WASM API (the `#[wasm_bindgen]` surface)

Each function takes the source string and returns a **JSON string** (parsed in JS). They never panic on
user error — a Phorge fault or checker error is captured into the JSON, not thrown. (The lib functions
already return `Result`; the wrappers map `Err`/fault into the JSON shape.)

```
pg_check(src)     -> { ok: bool, diagnostics: [ { code, message, line, col, span_len, severity } ] }
pg_run(src)       -> { ok: bool, stdout: string, fault: string|null, diagnostics: [...] }
pg_runvm(src)     -> { ok: bool, stdout: string, fault: string|null, diagnostics: [...] }
pg_transpile(src) -> { ok: bool, php: string|null, diagnostics: [...] }
```

`diagnostics[].code` (e.g. `E-FORCE-UNWRAP`) drives the clickable `phg explain` panel; the explain text
is exposed via a fifth helper `pg_explain(code) -> string` reusing `cli::explain`.

Single-snippet programs are `package Main;` single-file (the flat transpile path); no multi-file project
loading, no `vendor/`, no `phg build`.

## Data flow

1. User edits in CodeMirror (debounced ~300ms).
2. JS posts the source to a **Web Worker** holding the wasm module.
3. Worker runs `pg_check` → diagnostics panel (caret-underlined spans; click `E-xxx` → `pg_explain`).
4. Worker runs `pg_run`, `pg_runvm`, `pg_transpile`.
5. Main thread feeds `transpile.php` to **php-wasm** → PHP stdout.
6. Compare the three stdouts:
   - all equal → green **"3 backends agree"** badge;
   - `run` ≠ `runvm` → red **interpreter/VM divergence** banner + line diff;
   - Rust ≠ PHP → orange **transpile divergence** banner + line diff.
7. Backend tabs show `run` / `runvm` / `PHP output` / `PHP source`.

## Execution safety

- All Rust-backend execution runs **inside the Web Worker**; the main thread arms a ~5s watchdog. On
  timeout it `terminate()`s and recreates the worker and shows "execution timed out." This is the only
  defense against an infinite loop (wasm is single-threaded and non-interruptible).
- The existing recursion / nesting / expression-depth guards (`src/limits.rs`) cover stack blow-ups.
- php-wasm execution is likewise time-boxed on the main side (it runs async).
- No network, no filesystem, no eval of host code: the browser sandbox needs no augmentation.

## v1 features

- **Examples picker** — a build-time script reads `examples/guide/*.phg` and emits `examples.js` as a
  `{ name: source }` map. Examples that use `Core.File` (filesystem) are either given a **virtual
  in-memory fixture** or excluded; the build logs which were dropped (no silent truncation).
- **Shareable permalink** — source → browser-native `CompressionStream('deflate')` → base64url, stored in
  `location.hash`. Decoded on load. **No JS dependency** (native compression). Falls back to plain
  base64url if `CompressionStream` is unavailable.
- **Diagnostics + explain panel** — see WASM API.
- **Backend tabs + diff-on-mismatch** — see data flow.

## "Always latest phg" — CI / deploy

`.github/workflows/playground.yml`, triggered on push to `master` (and manual `workflow_dispatch`):

1. checkout, install Rust + `wasm-pack`,
2. `wasm-pack build playground --target web --release`,
3. run the examples-generation script,
4. assemble `dist/` (`index.html`, `main.js`, `style.css`, `examples.js`, the `pkg/` wasm output),
5. `actions/upload-pages-artifact` + `actions/deploy-pages`.

Every `master` push redeploys with the freshly compiled library — this is the "always latest" mechanism.
The existing `ci.yml` (tests + oracle + cross-build) is untouched; this is an additive workflow.

## php-wasm delivery

v1 loads php-wasm from the jsDelivr CDN at runtime — leanest, keeps the repo small; the tradeoff is a
runtime CDN fetch (no offline). Vendoring the multi-MB dist into the Pages artifact (fully self-contained,
offline-capable, larger repo) is the documented alternative, deferred unless offline is required.

## Testing & completion gate

- **Rust unit tests** on the wrapper functions (native target): assert the JSON shape, that a clean
  program reports `ok:true` with correct stdout, that a checker error populates `diagnostics`, and that a
  runtime fault is captured into `fault` (never a panic / wasm abort).
- **Byte-identity is NOT re-proven here** — it stays gated by `tests/differential.rs` at the Rust level.
  The playground only *surfaces* agreement; the diff banner is a UX affordance, not the correctness gate.
- **Visual evidence** (rendered-UI rule): build the bundle, serve it, and capture before/after
  screenshots of the running playground (editor + three agreeing backends) as Coverage evidence.
- **Optional**: one `wasm-pack test --headless` round-trip smoke test.

## Scope guard (YAGNI — explicitly out of v1)

No auth, no server-side save, no multi-file projects, no `vendor/` deps, no `phg build`, no real
`Core.File` I/O, no LSP/autocomplete (that is the separate IDE-tooling track). Single-snippet
`package Main;` programs only.

## Risks

1. **Workspace conversion** touches the root `Cargo.toml` — must verify `phg` bin, all lib tests, and
   `differential.rs` still build/pass unchanged after adding `[workspace]`.
2. **php-wasm version drift** — pin a php-wasm version that is PHP 8.4; a future 8.5 default could
   reintroduce the "local oracle too permissive" class of bug. Pin explicitly.
3. **`Core.File` examples** — must be filtered or virtualized so the examples picker never ships a broken
   sample.
4. **wasm size with php-wasm** — php-wasm is multi-MB; lazy-load it only when the user first runs (the
   Rust backends and diagnostics work without it).
```
