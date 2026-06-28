# Conformance corpus

A **golden-output** regression net for Phorj's *stable* surface (GA rock 3). Each program here has a
committed expected output; `tests/conformance.rs` asserts the interpreter, the bytecode VM, **and** the
transpiled PHP all produce *exactly* that output.

This is strictly stronger than the example differential (`tests/differential.rs`), which only checks
that the three backends *agree*: a regression where every backend drifts to the same wrong value passes
`agree` but fails here, because the golden pins the **value**. The corpus enumerates the constructs
listed as `stable` in [`../STABILITY.md`](../STABILITY.md), so a stable-surface regression is caught
loudly.

## Layout

- **`lang/`, `types/`, `collections/`, `stdlib/`, `errors/`** — small, single-feature programs. Each
  `<name>.phg` has a sibling `<name>.out` with its exact expected stdout.
- **`ddd/`** — a flagship multi-file, multi-package program (a Domain-Driven-Design ordering domain:
  bounded contexts → packages, entities/value-objects/aggregates → classes, folder = package path). It
  proves the features *compose at realistic scale*: cross-package `import type`, an aggregate computing
  over its entities, namespaced PHP emission. Its golden is `ddd/expected.out`; it loads through
  `loader::load` like any project.

## Discipline

- Output must be **deterministic** — no ambient state, no irrational floats (those diverge between the
  Rust backends' shortest-round-trip formatting and PHP's 14-digit `echo`; use exactly-representable
  values, e.g. integer cents for money).
- Discovery is glob-based: a program added under `conformance/` is gated with no test edit. A directory
  holding a `phorj.toml` is treated as a project (golden = `expected.out`).
- Run the full gate (incl. the PHP oracle) before relying on a change:
  `PHORJ_PHP=/path/to/php PHORJ_REQUIRE_PHP=1 cargo test --test conformance`.

## Regenerating a golden

After an *intended* output change, regenerate the affected `.out` and **read the diff** to confirm the
new output is correct (a golden that matches a wrong value is a silent lie):

```sh
phg run conformance/<area>/<name>.phg > conformance/<area>/<name>.out
```
