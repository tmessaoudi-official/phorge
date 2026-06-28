# Examples Backfill — guides + examples for already-shipped features

> Companion to the M3 work. **Standing rule** (developer, 2026-06-17): every feature we ship lands
> with a guide + example file(s). This backfills the gap for features shipped *before* that rule.

**Goal:** Cover the shipped features that currently lack a dedicated example/guide — functions,
`phg build`, and the CLI/diagnostics surface — so `examples/` is a complete showcase of the
"today" column of `FEATURES.md`.

**Gap** (FEATURES ✅ rows vs. `examples/` inventory, 2026-06-17): functions (only incidental, via
`fib`/`control-flow`), `phg build` (none), `phg explain`/diagnostics (none), stdin/`-e`/`--`
source forms (none), `check`/`parse`/`lex` (none). Arithmetic + its overflow-checking note already
live in `guide/operators.phg`, so no separate `checked-arithmetic.phg` — the clean fault is shown as
real output in the CLI guide instead.

**Verification spine:** every new `.phg` is globbed by `tests/differential.rs`, so it must run
byte-identically on `run` and `runvm` (and transpile cleanly). README walkthroughs use **real
captured** command output, never invented.

---

### Task 1: `guide/functions.phg` — focused functions guide
**Files:** Create `examples/guide/functions.phg`; Modify `examples/README.md` (index + matrix).
- [ ] Write the example: typed params, declared return type, a no-return function, composition (one
      function calling another), a `List<int>` return via a range, nested call results. Distinct
      focus from `control-flow.phg` (which owns recursion/branching/loops).
- [ ] Verify identical output: `phg run` matches `phg runvm`; `phg transpile` emits clean PHP.
- [ ] Add the README index row + coverage-matrix row.
- [ ] `cargo test` green (file auto-gated). Commit.

### Task 2: `examples/build/` — standalone-executable walkthrough
**Files:** Create `examples/build/app.phg`, `examples/build/README.md`; Modify `examples/README.md`.
- [ ] `app.phg`: a small self-contained program (fib loop) suitable to build.
- [ ] Capture real host output: build the binary to a temp path, run it, record stdout.
- [ ] README: the host `phg build` flow, what the `.phorj` section holds, the
      built-binary-matches-`runvm` parity-test pointer, and that cross/macOS targets are partial.
- [ ] Verify `app.phg` runs identically on both backends. `cargo test` green. Commit.

### Task 3: `examples/cli/` — source forms + inspection + diagnostics
**Files:** Create `examples/cli/demo.phg`, `examples/cli/README.md`; Modify `examples/README.md`.
- [ ] `demo.phg`: a tiny program for the walkthrough.
- [ ] Capture real output for: `run <file>`, `run -` (stdin), `run -e '<code>'`; `check`/`parse`/`lex`;
      `explain <CODE>`; an intentional compile error (caret diagnostic + did-you-mean + stable code);
      a runtime fault (÷0 or index OOB) proving "never panics".
- [ ] README walkthrough with that captured output. Verify `demo.phg` parity. `cargo test`. Commit.

### Task 4: docs + convention
**Files:** Modify `CLAUDE.md` (standing-rule convention line); create the feedback memory.
- [ ] Record "every shipped feature ships with a guide + example" as a project convention.
- [ ] Commit.
