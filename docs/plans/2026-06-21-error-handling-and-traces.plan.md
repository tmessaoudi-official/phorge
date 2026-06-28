# Error handling & stack traces — Plan

> A milestone-scale developer-experience effort. Two sequenced slices (developer-chosen): **(1)
> fault reporting — state-of-the-art stack traces — FIRST**, then **(2) a catchable error model**
> (try/catch vs Result, its own design pass). Research/brainstorm in progress.

## Decisions Log
- [2026-06-21] AGREED: triggered by the developer's "state-of-the-art error handling + stack traces"
  request. **Reframe (verified):** the whole-project ahead-of-time validation they also asked for
  ALREADY EXISTS — `phg run`/`check` merge every file + vendored dep and type-check before `main()`,
  so a broken class in an un-exercised route is structurally impossible. That part shipped as the
  `phg check` scope-summary polish (`4ffc0d5`). See [[error-handling-and-whole-project-validation]].
- [2026-06-21] AGREED: **scope = BOTH, traces first.** Slice 1 = better fault reporting (call stack,
  per-frame file:line, source caret) — pure DX, does NOT change the language. Slice 2 = a catchable
  error model (try/catch [PHP-native] vs Result<T,E> [type-safe]) — a language feature, its own design.
- [2026-06-21] CONSTRAINT (verified from code): the **VM keeps explicit call frames** (`Frame{func,ip}`)
  + per-instruction lines (`Chunk.lines`), so it can build a real trace directly. The **interpreter
  recurses on the native Rust stack** (only a `depth` counter) — it needs an ADDED logical call-stack
  to produce frames. Runtime faults are currently bare `String`s; no trace, no caret.
- [2026-06-21] CONSTRAINT: the byte-identity spine compares faults **semantically by `FaultKind`**, not
  raw text — so trace output lives on the **fault/stderr path** and never enters program stdout; the
  M7 PHP oracle is unaffected (PHP's own trace goes to stderr; FaultKind classification unchanged).

- [2026-06-21] AGREED: **traces are identical across backends (`run ≡ runvm`)** — the interpreter gains
  a logical call-stack mirroring the VM's frames, so one fault yields one trace regardless of backend.
- [2026-06-21] AGREED: **two presentation targets — CLI and web.** A shared structured `Trace`/`Fault`
  value (backend-produced, identical run≡runvm) feeds two renderers: (a) a polished CLI renderer
  (color/NO_COLOR, frames, source carets); (b) a browser **dev error page** shown when a `phg serve`
  app hits an uncaught fault (reuse the shipped XSS-safe `Core.Html` kernel to build it). The web page
  is **runtime glue** (like the M6 socket bridge), OUTSIDE the byte-identity value contract, and
  **dev-mode only** — production must return a generic 500 and never leak a trace/source (a GA M8
  security rule). Connects to [[m6-web-capabilities-direction]] and [[core-html-design]].

- [2026-06-21] AGREED: **deliver Slice 1 all-at-once** — CLI traces AND the web dev error page in one
  slice (developer chose "no compromise / complete" over phased). Larger single landing; design covers
  both renderers together.

- [2026-06-21] AGREED: **Slice-1 design approved** ("Approve — write the spec"). Spec:
  `docs/specs/2026-06-21-stack-traces-and-fault-reporting-design.md`. Shared `Fault` value;
  interpreter `trace_stack` + VM frame-walk (run≡runvm trace parity, harness-enforced); function→file
  tags + source map on `Unit` (extends visibility provenance); CLI renderer + dev-only web error page
  (Core.Html escaping discipline, runtime glue outside the oracle); prod = bare 500 no-leak. No new
  `Op`, stdout unchanged, FaultKind preserved.

- [2026-06-21] AGREED: **execute Slice 1 fully autonomously** — build all 8 impl tasks straight
  through, no per-task checkpoint, stop only on a genuine craftsmanship fork or a red gate (mirrors the
  visibility + mutation directives). `_AUTONOMOUS_3C=1`.

## STATUS (Slice 1 — in progress, 2026-06-21)
- **DONE + committed:** Task 1 `Frame`+`Diagnostic.frames`+CLI render (`3cc83fa`); Task 2 VM frame-walk
  (`d6a7230`); Task 3 interpreter `trace_stack` (`6cc563c`); **Task 5 trace-parity** — `run≡runvm`
  byte-identical traces, line backfilled from the innermost frame (`7a424ca`). 689 tests green.
  Frame names mirror the VM's compiled `Function.name` (`main`/`Class::method`/`Class::new`/`$set`).
- **Task 4 DONE** (`a6e72fa`): `Unit.sources`+`fn_files` + `attribute_frames`; `run_program`/
  `runvm_program` take `&Unit` and `render()` the fault (frames+caret reach the user; free fns + `main`
  get `file:line`).
- **Task 6 DONE** (`29b8b26`): `phg serve --dev` HTML error page (fault+stack+request, `Core.Html`
  escaping); production stays a bare 500 (no leak).
- **Task 7 SKIPPED** — Phorj diagnostics are plain (no ANSI anywhere); coloring only the trace would
  be inconsistent and coloring all diagnostics is out of scope. Deferred as polish (KNOWN_ISSUES).
- **Task 8 DONE:** `examples/errors/README.md` walkthrough + index; CHANGELOG / KNOWN_ISSUES /
  MILESTONES updated. **SLICE 1 COMPLETE** — 690 tests green. Slice 2 (catchable model) next.

## Formal Plan
Slice-1 implementation plan: **`docs/plans/2026-06-21-stack-traces-impl.plan.md`** — 8 tasks, TDD:
`Frame`+`Diagnostic.frames`+CLI render → VM frame-walk → interpreter `trace_stack` → loader
file-attribution+source-map → run≡runvm parity differential test → `phg serve --dev` web error page
(prod stays bare 500) → CLI color → docs/example. Each task ends green on the full
`PHORJ_REQUIRE_PHP=1` gate. Slice 2 (catchable error model — try/catch vs Result) is a later design.
