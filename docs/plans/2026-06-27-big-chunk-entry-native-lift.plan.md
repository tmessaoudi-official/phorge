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
Extended Phase 0 (harness purity already exists; sub-2^63 `Core.Random`) → Tier-A modules: Hash →
Encoding → Url → Validate → Csv. Each a gated guide example. **`Core.Http`** added here (absorbs the
web `respond` bridge so `handle` is directly servable — Batch-1 C remainder).

## Stage 3 — Bidirectional lift (L5 + L6)
L5 round-trip semantic gate (PHP→Phorge→PHP via oracle) + L6 `phg lift <file.php>` CLI.

## Status
Batch-1 B DONE (`b710c6e`). Next: Batch-1 C. Base `9fb9f32`. Autonomous; commit green, no push.

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
