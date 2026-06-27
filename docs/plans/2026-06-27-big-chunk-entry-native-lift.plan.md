# Big Chunk: Entry-point → Native stdlib → Bidirectional lift

## Decisions Log
- [2026-06-27] AGREED: build all three, in order **Entry-point (B+C) → Native stdlib wave → Bidirectional lift (L5+L6)** (developer: "all of them", chose recommended order). Rationale: foundation → breadth → capstone; lift last covers the widest stable surface (least rework).
- [2026-06-27] AGREED: pace = fully autonomous (autonomous-3c sentinels armed, per-session + per-project). Commit green byte-identical slices; never push.

## Stage 1 — Entry-point story

### Batch-1 B — `main(args: List<string>): int`  [IN PROGRESS]
Signature: `main` accepts **0 or 1** params; the one param must be `List<string>` (argv); return must be `void` or `int` (exit code). New diagnostic `E-MAIN-SIGNATURE`.
- **Checker** (`program.rs::check_function`): validate main shape → `E-MAIN-SIGNATURE`; `explain` entry.
- **Interpreter**: `interpret_main -> (String, i64)`; pass argv `Value::List` when main has a param; `int` return = exit code. `interpret` delegates (stdout only).
- **VM**: capture main's return value in `Op::Return` (frames==1) → `exit_value`; `run_main -> (String,i64)`; push argv as slot 0 when `main.arity==1`. `run` delegates.
- **CLI**: `run_program_exit`/`runvm_program_exit` (+ `cmd_run_exit`/`cmd_runvm_exit`); keep String variants for the differential. `main.rs` run/runvm sets `std::process::exit(code)`; built-binary path honors the code too.
- **Transpiler**: both bootstrap sites emit `[exit(]main([array_slice($argv??[],1)])[)];` per main's arity + return.
- **argv source**: reuse `PROCESS_ARGS` global (one source of truth — `Core.Process.args()` and `main(args)` agree). New `native::process::process_args_value() -> Value`.
- **Example**: `examples/guide/exit-codes.phg` (gated, `main(): int { …; return 0 }` — deterministic) + `examples/process/` argv-to-main walkthrough (quarantined, README).
- **Tests**: checker accept/reject; run≡runvm exit-code parity + argv→main parity (dedicated test); PHP exit parity where oracle available.

### Batch-1 C — formalize `handle(Request) -> Response` web entry
M6 W1 model already ships; mostly docs + `serve` convention. (Detail at stage entry.)

## Stage 2 — Native stdlib wave
Extended Phase 0 (harness purity already exists; sub-2^63 `Core.Random`) → Tier-A modules: Hash → Encoding → Url → Validate → Csv. Each a gated guide example.

## Stage 3 — Bidirectional lift (L5 + L6)
L5 round-trip semantic gate (PHP→Phorge→PHP via oracle) + L6 `phg lift <file.php>` CLI.

## Status
Batch-1 B in progress. Base commit `9fb9f32`. Autonomous; commit green, no push.
