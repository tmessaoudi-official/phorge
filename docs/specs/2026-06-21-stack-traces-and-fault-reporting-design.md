# Stack Traces & Beautiful Fault Reporting — Design (Error-handling Slice 1)

> **Status:** Designed — approved 2026-06-21, not yet implemented.
> **Milestone:** Error handling & stack traces, Slice 1 of 2 (Slice 2 = a *catchable* error model,
> its own design). Plan + Decisions Log: `docs/plans/2026-06-21-error-handling-and-traces.plan.md`.

## 1. Motivation

Today a runtime fault in Phorj is a single bare string (`Err(format!("cannot index {}", …))`) —
no call stack, no source caret, and the line is available only on the VM. For a language whose pitch
is *legible and provably-correct*, the uncaught-fault experience should be state-of-the-art: a real
**call stack** (function frames with `file:line`), a **source caret** at the failing line, rendered
beautifully in **two contexts** — the **CLI** (`phg run`/`runvm`) and the **web** (a `phg serve` app's
browser dev-error page).

A companion concern the developer raised — "a broken class in a route I haven't exercised hides until
that file runs" — is **already solved** and is not part of this slice: Phorj's loader merges every
`.phg` (first-party + vendored) and type-checks the whole program before `main()`, so such errors fail
up front (shipped as the `phg check` scope summary, `4ffc0d5`).

This slice is purely about **reporting faults that abort**. Catching/handling errors (`try`/`catch`
vs `Result<T,E>`) is Slice 2.

## 2. The `Fault` value — shared data model

One structured value, built identically by both backends, consumed by both renderers:

```rust
struct Fault {
    kind: FaultKind,        // the existing semantic classification (unchanged)
    message: String,        // today's bare error text
    site: Frame,            // exact fault location (innermost)
    frames: Vec<Frame>,     // call stack, innermost → outermost (site is frames[0])
}
struct Frame {
    function: String,       // e.g. "checkout", "main", "<closure>"
    file: Option<PathBuf>,  // origin file (None when unknown, e.g. -e/stdin)
    line: u32,
    col: u32,
}
```

Inner backend code keeps returning `Err(message)`; the **frame stack is attached at unwind** (top of
`run()`/`interpret()`), where the active frames are still known. `Fault` is the single source of truth
that guarantees CLI and web show the *same* trace.

## 3. Frame capture — making `run ≡ runvm` identical (the engineering crux)

The two backends are asymmetric, so each captures frames differently but must yield identical results.

- **VM:** at fault time `self.frames` is intact (the `Err` propagates up to `run()` before frames are
  discarded). Walk it: `Frame.func` → function name (from the program's function table),
  `Chunk.lines[ip]` → line. The top frame's `ip` is the faulting instruction; lower frames' `ip` sit
  at their pending call (the call-site line). No new machinery.
- **Interpreter:** add `trace_stack: Vec<Frame>` to the interpreter. Push a frame on call entry
  (function name + the call-site line); **update its current line** as the body's statements/expressions
  evaluate (so a fault deep in a body reports the right line); pop on **normal return only**. On an
  error, `?`-propagation **skips the pops**, so at the top-level catch `trace_stack` still holds every
  active frame → snapshot it into `Fault.frames`. This reproduces the VM's frame set and per-frame
  lines exactly.

**Parity is enforced, not hoped.** `tests/differential.rs` gains an assertion that `run` and `runvm`
emit **byte-identical trace text** for the same fault (it currently compares only `FaultKind`). That
test is the guardrail that keeps the interpreter's logical stack and the VM's real stack in lockstep.

Frame-line agreement rule (the subtlety): a non-top frame's line is its **call-site** line in both
backends (VM: `ip` paused at the `Call`; interpreter: the call-site line recorded at push). The top
frame's line is the **fault site** in both.

## 4. File + source attribution (surviving the loader merge)

The M5 loader flattens all files into one `Program` and leaves `diag_src` empty for merged units, so
spans carry line/col but **no file**. To attribute frames to files and pull source excerpts:

1. **Tag functions with their origin file** during loader Pass 1 — extend the provenance map already
   built for visibility (which records each definition's file) so every `FunctionDecl` (or its mangled
   name) maps to a `PathBuf`. A frame's `file` is resolved from its function name via this map.
2. **Keep a source map** `{ PathBuf → String }` on the loaded `Unit` (the texts already read in Pass 1).
   The renderer pulls a caret excerpt by `(file, line)`.

Loose mode (`-e`/stdin/single file) has one source and no file name → `file = None`, excerpt from the
single source. This is the "complete" path: full file+source attribution in multi-file projects, not
just single files.

## 5. CLI renderer

Rust-compiler-grade terminal output; **color only when stdout is a TTY**, honoring `NO_COLOR`
(reusing the existing `Diagnostic` color discipline). Layout:

```
error: list index out of range
  ┌─ src/acme/shop/cart.phg:14:11
14 │     return items[i];
   │            ^^^^^^^^ index 5, length 3
   │
stack trace (most recent call first):
  → checkout         src/acme/shop/cart.phg:14
    applyDiscount    src/acme/shop/cart.phg:31
    main             src/main.phg:6
```

Header (`kind` + `message`) → caret'd fault site (when source is available) → frame list
`function  file:line`, innermost first (the `→` marks the fault frame). Did-you-mean / hint text reuses
the existing `Diagnostic` machinery where a fault has a known remedy (e.g. force-unwrap → "prefer `??`
/ `?.`"). When source is unavailable (no file text), the caret block is omitted; the frame list still
prints.

## 6. Web dev error page (`phg serve`)

When a served handler hits an **uncaught** fault, `phg serve --dev` returns an HTML **debug page** (the
Whoops/Ignition equivalent): fault kind + message, each frame with a **source excerpt**, and the M6
**request context** (method, path, headers from the `Request`). It is rendered by a **Rust** function
in the serve runtime — *not* the Phorj `Core.Html` module (that is user-space) — but it **reuses
Core.Html's pinned escaping discipline** (the `htmlspecialchars(ENT_QUOTES)`-equivalent 5-char table)
so every interpolated value (message, source, header) is **XSS-safe by construction**.

This page is **runtime glue, outside the byte-identity value contract** — exactly like the M6 socket
bridge. It is produced by the serving runtime, never by the transpiled `handle(Request) -> Response`
value path, so it never reaches the differential oracle.

## 7. Production safety (a GA M8 security rule, baked in now)

The rich page is strictly **dev-mode** (`phg serve --dev`). In production:

- an uncaught handler fault returns a **bare generic 500** — no stack trace, no source, no message;
- the transpiled PHP app's front-controller inherits the same rule (dev page only behind a dev flag).

Leaking traces/source in production is precisely what M8 hardening forbids; this slice bakes the
dev/prod split in from the start rather than retrofitting it.

## 8. Byte-identity & oracle safety

- Traces are written to the **fault/stderr path**; program **stdout is unchanged**.
- `FaultKind` classification is **unchanged**, so the M7 PHP oracle still matches faults semantically
  (PHP's own trace goes to its stderr and is ignored by the comparison).
- New invariant enforced by the harness: **`run`-trace ≡ `runvm`-trace** (§3).
- No new `Op`; no `Value` change; the web page is outside the value contract.

## 9. Testing

- **Unit:** `Fault` construction; CLI renderer golden-output tests (with and without source); web
  renderer HTML snapshot asserting every interpolated value is escaped.
- **Differential:** a set of representative faults — integer div-by-zero, list index-out-of-range,
  force-unwrap of null, map-key-not-found, deep recursion (stack overflow) — assert **identical trace
  text** across `run`/`runvm`, and identical `FaultKind` against real PHP.
- **Examples:** a faulting program cannot be a byte-identical runnable example (it aborts with no
  stdout), so it is captured as a differential test + a README walkthrough showing the CLI trace and
  the web page (per the examples-ship-with-features rule).

## 10. Scope & non-goals

- **In scope:** uncaught-fault traces (CLI + web), file/source attribution across multi-file projects,
  `run ≡ runvm` trace parity, the dev/prod web split.
- **Out of scope (Slice 2 — separate design):** a *catchable* error model — `try`/`catch` (PHP-native
  exceptions) vs `Result<T,E>` (type-system errors). This slice does not let a program intercept a
  fault; it only reports faults that abort.
- **Deferred (note, not blocker):** colored/syntax-highlighted source in the web page beyond escaping;
  a "cause chain" (needs Slice 2's error values); collapsing deep identical recursion frames in the
  trace (print a count) — a polish follow-on.

## 11. Blast radius

`interpreter.rs` (`trace_stack` + per-frame current-line), `vm.rs` (frame-walk at fault), new
`src/fault.rs` (the `Fault`/`Frame` types + CLI renderer), `loader.rs` (function→file tags + source
map on `Unit`, extending the visibility provenance), `serve.rs` (dev-gated web error page), `cli.rs` /
`main.rs` (route faults through the renderer), `tests/differential.rs` (trace-parity assertion). **No
new `Op`, no `Value` change, no change to program stdout, `FaultKind` preserved.**
