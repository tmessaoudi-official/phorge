# Process I/O & the "impure native" quarantine seam — Design

> ## ✅ DECISIONS LOCKED (developer, 2026-06-25)
> - **Q1 — `pure: bool` field on `NativeFn`** (default `true`; `Core.Env`/`Core.Process` set `false`).
> - **Q2 — skip impure programs entirely** from the byte-identity differential; test them in a dedicated
>   `tests/process.rs` with a controlled env + a `examples/process/` README walkthrough (not gated).
> - **Q3/Q4/Q5 — my recommended defaults:** argv via a process-global set before run (b); `Env.all()`
>   returns a **sorted** `Map` (by key); careful `--` terminator grammar in `cli::resolve_source`.
>
> Status: **APPROVED — implementing** (after Core.Reflect).
>
> Status (orig): **DESIGN — awaiting developer decisions.** Origin: Phase 2 Slice 3 of
> `docs/specs/2026-06-24-introspection-strings-process-design.md`, parked autonomously 2026-06-25 as
> fork **F-007**. This is "the M-Batteries kickoff" — it introduces the first natives whose results
> depend on the *environment*, so it needs a seam that keeps them off the byte-identity differential.

## Goal

```
Core.Process.args() -> List<string>       // program arguments (after the script), PHP $argv (sliced)
Core.Env.get(name: string) -> string?     // one env var or null, PHP getenv()
Core.Env.all() -> Map<string, string>     // all env vars, PHP $_ENV / getenv()
```

CLI: `phg run f.phg -- arg1 arg2` passes `arg1 arg2` to `Core.Process.args()`.

## Why a quarantine seam (the determinism analysis)

Within a single process the env/args are stable, so `run ≡ runvm` actually holds. The risk is the **PHP
leg**: the oracle transpiles + runs the program under a *separate* `php` subprocess, whose environment
and `$argv` are not guaranteed identical to the Rust process (PWD, injected vars, arg framing). Asserting
byte-identical stdout across all three for an env/args-reading program is therefore fragile *and* the
output isn't a fixed golden (it depends on the machine). Rather than chase env-equalization, these
natives are **quarantined**: excluded from the byte-identity differential, tested separately with a
controlled environment. (Precedent: `src/serve.rs` is tested outside `differential.rs`.)

## The seam — three mechanism options (DECISION REQUIRED, Q1)

A native must be marked "impure" so the harness can detect and skip programs that use it.

- **Q1-A — a `pure: bool` field on `NativeFn`** (default `true`; Process/Env set `false`). Simplest;
  one bool; the `eval` stays `Pure(...)` (env access is still a `fn(&[Value], &mut String)` — it just
  reads `std::env`). The harness reads the flag. *Recommended.*
- **Q1-B — a new `NativeEval::Impure(...)` variant.** Heavier (a 4th dispatch arm in both backends) and
  conflates "how it computes" with "is it deterministic" — env access is computationally just a `Pure`
  read. Not recommended.
- **Q1-C — a dedicated module allowlist** (`Core.Process`/`Core.Env` hardcoded in the harness). Brittle
  (every new impure module must be remembered). Not recommended.

## How the differential skips impure programs (Q2)

`tests/differential.rs` globs `examples/**/*.phg` and asserts `run ≡ runvm ≡ PHP`. It must skip a program
that calls an impure native. Mechanism: a small `program_uses_impure_native(&Program) -> bool` (walk the
AST for a `Member`/qualified call resolving to a native whose `pure == false`). Two sub-options:

- **Q2-A — skip the program entirely** from the byte-identity glob (don't run any backend on it in the
  differential). Process/Env examples live under e.g. `examples/process/` and are documented as
  *walkthroughs*, not gated examples (matching the spec's "README walkthrough, not a gated example").
- **Q2-B — run `run ≡ runvm` but skip the PHP leg** (the Rust backends share an env, so they still
  agree; only the PHP subprocess env is unreliable). Keeps *some* gating. Slightly more harness logic.

*Recommendation: Q2-A* (cleanest; matches the spec's intent that these are walkthroughs). A dedicated
`tests/process.rs` then tests the natives with a controlled env/args (set a known var, assert the value).

## CLI argv threading (Q3)

`phg run f.phg -- a b c`: the `--` terminator splits phg's own flags from the program's args.
`cli::resolve_source` already understands `--` as "next arg is a literal path"; this extends it to
"everything after `--` is the program's argv". Threading:

- A new `args: Vec<String>` on the interpreter and the VM (default empty), populated from the CLI.
- `Core.Process.args()` reads it (a `Reflective`-like seam, OR — simpler — the args are injected into a
  process-wide `OnceCell`/passed via the eval context). **Sub-decision Q3:** how does a `Pure` native
  reach the argv? Options: (a) a `&Context` param threaded like `ClassTables` (cleanest, but adds a
  param to the impure eval signature); (b) a process-global set before `interpret`/`vm.run` (simplest,
  but global state — acceptable since a `phg run` is one program one process). *Lean (b) for args+env:
  env reads `std::env` directly (already global); args set a `thread_local`/`OnceCell` before run.*
- For a **standalone built binary** (`phg build`), `Core.Process.args()` reads the real process `argv`
  (the embedded program runs as a normal executable). For the transpiled **PHP**, `$argv` / `getenv()`.

## Scope / non-goals (unchanged from the origin spec)

- **Included:** `Process.args`, `Env.get`, `Env.all`.
- **Rejected (ambient superglobals — the thing Phorj removes):** `$_REQUEST`, implicit `$_SERVER`
  access, `$_SESSION` (stateful, M6+). `$_GET`/`$_POST`/`$_FILES`/`$_COOKIE` → M6 `Request`, not here.
- No env *mutation* (`putenv`) in v1 — read-only, matching the reflection read-only stance.

## Open questions (gate the build)

- **Q1** — impure marker: `pure: bool` field (A), `NativeEval::Impure` (B), or module allowlist (C)? *Rec A.*
- **Q2** — differential skip: skip-program-entirely (A) or run-Rust-only-skip-PHP (B)? *Rec A.*
- **Q3** — argv reach into a native: threaded context (a) or process-global set before run (b)? *Rec b.*
- **Q4** — `Env.all()` ordering: env var order is OS-dependent; return a **sorted** `Map` (by key) for a
  stable result, or insertion/OS order? *Rec sorted* (consistency with the reflection-list decision).
- **Q5** — does `phg run`'s `--` terminator interact with the existing `-`/`-e`/`--` source forms in
  `cli::resolve_source`? (Needs a careful grammar pass so `phg run -- file.phg -- a b` is unambiguous.)

## What I did NOT decide

The seam touches `differential.rs` (the correctness harness) — a wrong skip could let a non-deterministic
program silently onto the oracle and cause flaky CI. So I parked it for your Q1/Q2 calls rather than
improvise the harness change. On your answers I build the seam + the three natives + a `tests/process.rs`
with a controlled env, and a `examples/process/` walkthrough (README, not gated).
