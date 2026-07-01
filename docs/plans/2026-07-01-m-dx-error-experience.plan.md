# M-DX — Error Experience & Build Profiles Plan

> Design spec (locked): `docs/specs/2026-07-01-error-experience-build-profiles-design.md` (`3a85c30`).
> Autonomous full-milestone sweep. Build straight from the spec + running decisions log (no per-slice
> plan files). Slices ship independently green: `cargo test --workspace` + clippy + `fmt --check` +
> PHP-8.5 oracle byte-identity + guide example, then commit.

## Decisions Log
- [2026-07-01] AGREED: build the **full milestone, all 6 slices** (S1→S0→S2→S3→S4→S5) in one
  autonomous sweep; commit each green; stop only on a genuine design fork or an unresolvable red gate.
- [2026-07-01] AGREED: **no per-slice plan files** — build straight from the locked design spec, keep a
  lightweight decisions log here. Be vigilant for surprises; improve existing code in passing or flag
  it if not solid enough.

## Slice tracker
- [x] **S1** Diagnostics quality — DONE. Soundness fixes B/C/D + D' (E-OVERRIDE-SIG return covariance,
      E-DUP-VARIANT, E-DUP-STATIC, E-DUP-CONST); 2 uncoded → coded (E-DUP-TYPE, E-TYPE-ARG-COUNT);
      **24** explain entries added (audit said 14 — the coverage ratchet found 10 more: all four
      E-TYPE-IMPORT-*, the E-DECL-* pair); diagnostic-coverage ratchet
      (`every_emitted_diagnostic_code_has_an_explanation`) + removed the drift-prone hardcoded fallback
      list; golden-diagnostic corpus (`conformance/diagnostics/` + `tests/diagnostics.rs`, bless-able).
      Full workspace green at PHP-8.5 floor, clippy+fmt clean. Corpus-per-all-codes = flagged future
      work (seeded with slice-touched codes only).
- [x] **S0** Build profiles — DONE. `profile::Profile { Dev, Release }` (`src/profile.rs`) + process
      `set_active`/`active` SSOT. `phg build` Release-by-default / `--dev` opt-in, baked into the
      `.phorj` container `flags` byte (bit 0, backward-compatible — pre-profile artifact = Release).
      `serve --dev` refolded onto Profile. Keystone verified: Dev vs Release build → byte-identical
      output. Tests: profile unit + container round-trip + serve dev/release page + build-artifact
      round-trip & output-invariance. Deferred: run/runvm rely on the Dev default (no explicit
      set_active — no consumer yet); the "env var can't flip Release→Dev" test defers to S3 (needs an
      observable Dev-only behavior); build embedding bytecode-not-source is its own follow-up.
- [x] **S2** Secure value renderer — DONE. `src/inspect.rs`: `render(&Value)`/`render_with(caps)`,
      `RenderCaps { max_depth, max_elements, max_scalar_bytes }`. Secret redaction (recognizes the
      injected `Secret` wrapper class, redacts without descending, incl. nested), bounded (depth/elem/
      byte caps with `…`), deterministic (insertion-ordered Map/Set, slot-ordered fields, no
      addresses/Rc-counts). Every `Value` variant covered; opaque handles → `<function>`/`<channel>`/
      `<task>`. 10 unit tests. Internal substrate — no CLI/example yet (ships with S3/S5).
- [x] **S3** Value-dump on fault — DONE. `phg run --dump-on-fault` dumps the faulting frame's named
      locals (via S2 `inspect`) to stderr after the backtrace; Dev + opt-in (`dump::should_dump` =
      enabled ∧ Dev; absent in Release). Secret-redacted, capped, sorted (deterministic). Interpreter
      captures locals at the innermost `run_call` (before scope unwind, `#[cold]` helper); VM shares
      the byte-identical backtrace, no locals (no slot→name table — S5-consistent). `Diagnostic.dump:
      Option<Box<String>>` (out-of-spine). Walkthrough `examples/dump/README.md`; dump unit + cli e2e
      tests. **DECISION: interpreter-rich locals, not byte-identical VM named-locals** (deviation from
      S3's literal "byte-identical dump" — resolved per the locked S5 interpreter-only precedent; flag
      for review). **GOTCHA: `Box<str>` is a 16-byte fat pointer** — using it on `Diagnostic` grew the
      hot recursive `Signal` frame enough to overflow the 256 MB `MAX_CALL_DEPTH` worker; `Box<String>`
      (thin 8-byte) fits. Verified pre-S3 parent was clean → confirmed the regression + fix.
- [x] **S4** Assertions — DONE (feature was pre-existing). `assert(cond[, msg])` already: checker-
      validated (bool + optional literal msg), `FaultMsg::Assert` on both backends (always-checked —
      keystone satisfied, never stripped in Release), FaultKind-classified in the differential
      (pass+fail tested `differential.rs:2279`), transpiles to a real PHP `throw` (not disableable
      `assert()`). M-DX added `examples/guide/assertions.phg` (byte-identical) + README matrix.
      **DECISION: no separate Dev-rich assert message** — operand inspection on a failing assert is
      already delivered by S3 `--dump-on-fault` (a failing assert is a `Signal::Runtime` fault); a
      second operand path would be redundant + interpreter/VM-asymmetric + a spine risk. Message stays
      uniform across profiles (byte-identical).
- [ ] **S5** Interactive debugger (interpreter-only; REPL + DAP frontends) — **NOT STARTED** (largest
      slice; deferred to a fresh session for a well-tested build). Ready-to-execute plan below.

### S5 implementation plan (ready to execute)
**Engine (foundation) — `src/debug.rs`:**
- `enum StepMode { Continue, StepInto, StepOver(usize depth), StepOut(usize depth) }`.
- `struct Debugger { breakpoints: BTreeSet<u32>, mode: StepMode }` with `should_pause(line, depth) -> bool`
  (Continue→line∈breakpoints; StepInto→always; StepOver→depth<=target ∨ bp; StepOut→depth<target ∨ bp)
  and `apply(cmd, depth)`.
- `enum DebugCommand { Continue, StepInto, StepOver, StepOut, SetBreakpoint(u32), ClearBreakpoint(u32), Quit }`.
- `struct PauseCtx { line: u32, depth: usize, locals: Vec<(String,Value)>, frames: Vec<Frame> }`.
- `trait DebugFrontend { fn on_pause(&mut self, ctx: &PauseCtx) -> DebugCommand; }` (REPL impl reads
  stdin/writes stderr; a **test** impl returns scripted commands → deterministic step-sequence tests).
- Reuse S2 `inspect::render` for locals + `snapshot_frames` for the backtrace.
**Interpreter hook — `src/interpreter/`:**
- Add `debug: Option<(Debugger, Box<dyn DebugFrontend>)>` to `Interp` (3 construction sites:
  `interpret_main` mod.rs:270, mod.rs:395, `Interp::for_task` coop.rs:35 — non-debug = `None`).
- Hook at `exec_stmt` (stmt.rs:6, right after the trace-line update): if debug attached and
  `should_pause(line, self.depth)`, call a `#[cold] #[inline(never)]` `debug_pause` (same hot-frame
  discipline as S3's `capture_fault_dump`) that loops `on_pause` (applying breakpoint-set commands,
  re-prompting) until a step/continue/quit command; quit → `rt("debug: quit")`.
- New entry `interpret_debug(program, frontend)` mirroring `interpret_main`.
**REPL frontend — `src/debug.rs` or `src/cli/debug_repl.rs`:** commands `break <line>`/`b`,
  `step`/`s`, `next`/`n`, `stepout`/`o`, `continue`/`c`, `locals`/`l`, `backtrace`/`bt`, `quit`/`q`.
  Prints pause context (line, locals via `inspect`, backtrace) to **stderr**; program output stays on
  stdout.
**DAP frontend — `src/dap.rs`:** model the transport on the existing **LSP stdio JSON framing**
  (`src/lsp/` — Content-Length headers + JSON). Core requests: `initialize`, `launch`,
  `setBreakpoints`, `configurationDone`, `threads`, `stackTrace`, `scopes`, `variables`, `continue`,
  `next`, `stepIn`, `stepOut`, `terminated`/`exited`. Runs the interpreter on a worker thread; the DAP
  loop + interpreter communicate via channels (a `DebugFrontend` impl that blocks on a channel).
**CLI — `src/main.rs` + `src/cli/`:** `phg debug <file>` (REPL) and `phg debug --dap <file>` (DAP
  server). Dev-only (`profile::active().is_dev()`); add to USAGE + `help_for`.
**Tests:** engine step-sequence tests (scripted frontend over a fixture); DAP protocol round-trip
  (build a request, feed bytes, assert response framing/fields); REPL command parsing. **Never touches
  the differential spine** (debugger is stderr/side-channel, interpreter-only).
**Docs/example:** `examples/debug/README.md` walkthrough (a debug session is interactive, not a
  runnable "Ok" example — README per the examples rule) + CHANGELOG + KNOWN_ISSUES (VM stepping,
  conditional breakpoints, watchpoints, hot-reload deferred per spec).

## Surprises / improvements flagged
- [S1] The W1 audit's "14 missing explain" undercounted: the coverage ratchet (a source scan) found
  **24** — it caught `E-TYPE-IMPORT-{BUILTIN,CONFLICT,SHADOW,UNKNOWN}` and `E-DECL-{PACKAGE,NONFOREIGN}`
  that the manual audit missed. Lesson: a mechanical ratchet beats a hand audit for completeness.
- [S1] `E-DECL-*` codes live *inside* multi-line `format!` strings in the loader (plain `String`
  errors, not `Diagnostic`) — the ratchet scanner had to go whole-file (not per-line) to see them.
  Loader errors being a separate `String` channel (no `.with_code`, no caret) is an inconsistency worth
  a future slice (migrate loader to `Diagnostic`).
- [S1] The hardcoded "known codes" list in the `explain` fallback had already drifted (missing the 24).
  Removed it entirely; the ratchet is the SSOT guarantee now.
- [S3] **`Box<str>` is a fat pointer (16 bytes), `Box<String>` is thin (8 bytes).** Putting a
  `Option<Box<str>>` on `Diagnostic` (the interpreter's hot recursive `Signal` payload) grew the
  per-frame stack enough that a `MAX_CALL_DEPTH`-deep recursion overflowed the 256 MB deep-stack worker
  *before* the depth guard fired — a `SIGABRT` in `tests/differential.rs`. `git stash` + testing the
  parent proved it was an S3 regression; `Box<String>` fixed it. Lesson: adding ANY field to
  `Diagnostic`/`Signal` must stay within the frame-size margin; prefer a thin boxed pointer.
- [S3] **DEVIATION FROM SPEC (flag for review):** S3's literal design says the dump is "byte-identical
  between backends." The VM has no slot→name table, so byte-identical *named* locals would be a
  debug-symbol subproject. Resolved per the already-locked S5 decision (debugger is interpreter-only
  because the spine guarantees the backends agree): rich locals on the interpreter, byte-identical
  backtrace on both. Alternative if the developer wants true VM parity: build a per-scope VM
  debug-symbol table (larger; contradicts the S5 rationale).
