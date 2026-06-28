# Phorj M1 — Plan 6: CLI Runner (Design)

> Status: **frozen** (2026-06-15). Final M1 plan (6/6). Inputs: the frozen
> language design (§6 sample) and all prior stage APIs (`lexer::lex`,
> `parser::Parser`, `checker::check`, `interpreter::interpret`).

## 1. Goal

`phg run file.phg` executes a Phorj program end-to-end and prints its output.
The full four-stage pipeline (lex → parse → check → interpret) is exposed through
subcommands. After this plan, M1 is complete: programs run from a file.

## 2. Architecture

A thin `src/main.rs` dispatcher delegates to a new **`src/cli.rs`** library module.
Keeping the pipeline and error rendering in the library (not `main`) makes them
unit-testable without spawning the binary; `main` only does arg parsing, file I/O,
printing, and exit codes. **Std only** — no argument-parsing crate.

The `[[bin]] phorj` and `src/main.rs` already exist (a `lex` debug command from
Plan 1); this plan extends them.

## 3. Commands

`phg <command> <file>`:

| Command | Pipeline | Success output |
|---------|----------|----------------|
| `run`   | lex → parse → **check (gate)** → interpret | the program's stdout buffer |
| `check` | lex → parse → check | `OK (type-checks clean)` |
| `parse` | lex → parse | AST dump via `{:#?}` |
| `lex`   | lex | token dump (existing behaviour, preserved) |

`run` enforces the type-checker as a gate: if `check` returns errors, they are
printed and the program is **not** executed.

## 4. Files

- `src/cli.rs` — `cmd_run`, `cmd_check`, `cmd_parse`, `cmd_lex` (each
  `fn(&str) -> Result<String, String>`: `Ok` = text to print verbatim, `Err` =
  rendered error message), plus private `lex_parse` / `parse_checked` helpers.
  Unit tests.
- `src/main.rs` — rewritten as a thin dispatcher over `phorj::cli`.
- `src/lib.rs` — add `pub mod cli;`.
- `tests/cli.rs` — subprocess smoke tests via `env!("CARGO_BIN_EXE_phg")`.
- `tests/fixtures/sample.phg` — the verbatim §6 sample (committed fixture).

## 5. Error reporting

Each stage error renders to one human-readable line. `LexError`, `ParseError`,
and `TypeError` all carry `line`+`col`; `RuntimeError` carries only `message`
(by design, EV-3):

- lex:   `lex error at L:C: <msg>`
- parse: `parse error at L:C: <msg>`
- type:  `type error at L:C: <msg>` — **one line per error**, all of them
- run:   `runtime error: <msg>`

## 6. Exit codes

- `0` — success
- `1` — compile error (lex/parse/type) or runtime error
- `2` — usage error (bad/missing subcommand) or unreadable file

`Ok(text)` is written to **stdout** with `print!` (the text carries its own
trailing newline — the interpreter's `out` buffer is already newline-terminated;
the other commands append one). `Err(msg)` is written to **stderr** via `eprintln!`
and exits `1`. Usage/IO failures are handled in `main` and exit `2`/`1`.

## 7. Testing

**Unit** (`src/cli.rs`): `cmd_run` on the §6 sample → exact three-line output;
`cmd_run` on a type-error program → `Err` containing `type error`; `cmd_run` on a
div-by-zero program → `Err` containing `runtime error`; `cmd_check` clean → `Ok`
with `OK`; `cmd_check` on a type error → `Err`; `cmd_parse` → `Ok` containing
`Program`; `cmd_lex` → `Ok` containing a token.

**Subprocess** (`tests/cli.rs`, std `Command`, no external crates):
`phg run tests/fixtures/sample.phg` → stdout == the three lines, exit 0;
no-args → exit 2; `run` on a nonexistent file → exit 1.

## 8. Decisions Log

- **CLI-1** Subcommand invocation (`phg <cmd> <file>`), not bare-file default —
  consistent with the existing `lex` command.
- **CLI-2** Commands: `run`, `check`, `parse`, `lex` (all four).
- **CLI-3** Thin `main` over a testable `cli` library module; std-only arg parsing.
- **CLI-4** `run` type-checks first (gate); refuses to execute on type errors.
- **CLI-5** Exit codes `0`/`1`/`2` (success / compile-or-runtime error / usage-or-IO).
- **CLI-6** PHP converter is a **separate future milestone** (Phorj→PHP transpile
  first; PHP→Phorj import deferred and evaluated separately) — explicitly out of
  Plan 6 scope.
