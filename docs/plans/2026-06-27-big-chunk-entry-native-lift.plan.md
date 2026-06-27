# Big Chunk: Entry-point → Native stdlib → Bidirectional lift

## Decisions Log
- [2026-06-27] AGREED: build all three, in order **Entry-point (B+C) → Native stdlib wave → Bidirectional lift (L5+L6)** (developer: "all of them", chose recommended order). Rationale: foundation → breadth → capstone; lift last covers the widest stable surface (least rework).
- [2026-06-27] AGREED: pace = fully autonomous (autonomous-3c sentinels armed, per-session + per-project). Commit green byte-identical slices; never push.

## Stage 1 — Entry-point story

### Batch-1 B — `main(args: List<string>): int`  [DONE — `b710c6e`]
Signature: `main` accepts **0 or 1** params; the one param must be `List<string>` (argv); return must be `void` or `int` (exit code). New diagnostic `E-MAIN-SIGNATURE`.
- **Checker** (`program.rs::check_function`): validate main shape → `E-MAIN-SIGNATURE`; `explain` entry.
- **Interpreter**: `interpret_main -> (String, i64)`; pass argv `Value::List` when main has a param; `int` return = exit code. `interpret` delegates (stdout only).
- **VM**: capture main's return value in `Op::Return` (frames==1) → `exit_value`; `run_main -> (String,i64)`; push argv as slot 0 when `main.arity==1`. `run` delegates.
- **CLI**: `run_program_exit`/`runvm_program_exit` (+ `cmd_run_exit`/`cmd_runvm_exit`); keep String variants for the differential. `main.rs` run/runvm sets `std::process::exit(code)`; built-binary path honors the code too.
- **Transpiler**: both bootstrap sites emit `[exit(]main([array_slice($argv??[],1)])[)];` per main's arity + return.
- **argv source**: reuse `PROCESS_ARGS` global (one source of truth — `Core.Process.args()` and `main(args)` agree). New `native::process::process_args_value() -> Value`.
- **Example**: `examples/guide/exit-codes.phg` (gated, `main(): int { …; return 0 }` — deterministic) + `examples/process/` argv-to-main walkthrough (quarantined, README).
- **Tests**: checker accept/reject; run≡runvm exit-code parity + argv→main parity (dedicated test); PHP exit parity where oracle available.

### Batch-1 C — formalize `handle(Request) -> Response` web entry  [DONE — satisfied + scoped]
Already shipped + formalized: M6 W1 (`handle`/`Request`/`Response`/parse/serialize) + W2 router + W4
`phg serve` (runs `respond(bytes)->bytes`), documented as a contract in `examples/web/README.md` and
listed ✅ in `FEATURES.md:73`. **Decision (autonomous):** the only remaining enhancement — `phg serve`
running a bare `handle` without the per-app `respond` bridge — REQUIRES a standard `Core.Http`
(Request/Response/parse/serialize); synthesizing the bridge in Rust would leak HTTP policy
(malformed→400) into the runtime and break the determinism layering. So it **folds into Stage 2** as a
`Core.Http` module. Recorded the deferral in `examples/web/README.md`. No code this slice.

## Stage 2 — Native stdlib wave  [ACTIVE]
Extended Phase 0 (harness purity already exists; sub-2^63 `Core.Random`) → Tier-A modules. Each a
gated guide example. **`Core.Http`** added here (absorbs the web `respond` bridge so `handle` is
directly servable — Batch-1 C remainder).
- **`Core.Encoding`** — base64 + hex (encode `bytes->string`, decode `string->bytes?`). DONE `31745c3`.
- **`Core.Hash`** — crc32/md5/sha1/sha256 (hand-rolled, `bytes->string` hex). DONE `8b8896f`.
- **`Core.Url`** — urlEncode/urlDecode/rawUrlEncode/rawUrlDecode (percent-encoding; decode `->string?`
  null on invalid-UTF-8). DONE `fe5ef1e`.
- **`Core.Validate`** — isInt/isNumber/isAlpha/isAlnum/isHex (`string->bool`, hand-roll + matching PHP
  `preg_match`). DONE `08eb5e5`.
- Next: `Core.Csv` (parse line `string->List<string>` / format `List<string>->string`; match PHP
  `str_getcsv`/`fputcsv` quoting — FIDDLY, pin every quoting edge to php) → `Core.Random` (QUARANTINED
  — seeded PRNG, constants `<2^63`, shifts `1..=63`, no PHP-float `/`; examples in `examples/random/`
  like process) → `Core.Http` (Request/Response/parse/serialize → makes `handle` directly servable,
  closes Batch-1 C remainder; the biggest module).
Pattern: `src/native/<m>.rs` (`Vec<NativeFn>` + `php:` emission) + register in `native/mod.rs` +
`#[path]` unit tests + a gated `examples/guide/<m>.phg` + README row. Tier-A only if byte-identical to
a PHP **core** fn under `php -n` (no mbstring; hash/base64/bin2hex/pcre are core).

## Stage 3 — Bidirectional lift (L5 + L6)
L5 round-trip semantic gate (PHP→Phorge→PHP via oracle) + L6 `phg lift <file.php>` CLI.

## Status
**Stage 1 DONE** (`b710c6e` Batch-1 B, `6f0a939` Batch-1 C). **Stage 2 in progress**: Encoding `31745c3`,
Hash `8b8896f`, Url `fe5ef1e`, Validate `08eb5e5` done; next = Csv → Random → Http. **Stage 3 (lift
L5/L6)** not started. Base `9fb9f32`; 7 commits this session, all green, **unpushed** (awaiting push).
Autonomous; commit green, no push.

### Native-module recipe (reuse for Url/Validate/Csv/Http)
1. `src/native/<m>.rs`: `<m>_natives() -> Vec<NativeFn>` (each: `module:"Core.X"`, `name`, `params`,
   `ret`, `pure:true`, `eval: NativeEval::Pure(fn)`, `php: |a| ...` using `parg(a,i)`).
2. Register: `mod <m>;` + `registry.extend(<m>::<m>_natives());` in `src/native/mod.rs`.
3. `#[cfg(test)] #[path="<m>_tests.rs"] mod tests;` — pin kernels to **real `php -n` output**.
   (`Value` has NO `PartialEq` → compare via `matches!` / extract fields.)
4. Gated `examples/guide/<m>.phg` + a row in `examples/README.md`. Tier-A only if byte-identical to a
   PHP **core** fn under `php -n` (hash/base64/bin2hex/pcre are core; mbstring is NOT — see
   [[transpile-no-ini-extensions]]). Quarantine impure modules (import-based, like `Core.Process`).
5. Gate: `PHORGE_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php PHORGE_REQUIRE_PHP=1 cargo test
   --workspace` + `cargo clippy --all-targets -- -D warnings` + `cargo fmt --all --check`. Commit green.

### Batch-1 B notes (for reuse)
- `interpret`/`Vm::run` kept stdout-only (delegate to `interpret_main`/`run_main` returning
  `(String,i64)`) — preserved hundreds of `agree`/oracle call sites untouched. Exit code: interpreter
  reads `run_call`'s `Ok(v)` (it converts `Signal::Return` to `Ok`); VM stashes `exit_value` in
  `Op::Return` when `frames.len()==1` (do_return drops it once stack empties).
- argv single-sourced via `native::process_args_value()` (same value `Core.Process.args()` returns);
  VM pushes it as slot 0 when `main.arity==1`.
- `run_php` asserts exit-0 → a gated example must `return 0`; non-zero exit parity is tested by driving
  php directly (`out.status.code()`). argv examples are quarantined (import `Core.Process`).
- Two argv-setting tests race the `PROCESS_ARGS` global → serialize with a `Mutex` (poison-tolerant).
