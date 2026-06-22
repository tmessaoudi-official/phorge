# error-model-slice2 Plan (M-faults Slice 2)

The three-tier error model: enforced typed **`throws E`** (→ idiomatic PHP exceptions), **`Result<T,E>`**
value surface (the shipped generic enum + `?`), and unchecked **faults/panics** (crash + Slice-1 stack
trace, never declared up-chain). Byte-identical `run ≡ runvm ≡ real PHP`. Design-first (brainstorming),
then writing-plans.

## Decisions Log
- [2026-06-22] AGREED: next slice = **error model (Slice 2)** over method overloading — biggest GA
  lever, unblocked now that generic enums ship (`Result<T,E>` is expressible), completes the `never`
  story (`throw`/`panic` become the real `never` producers).
- [2026-06-22] AGREED: **design the full three-tier model in one spec**; build cadence (one-shot vs
  sub-sliced) **deferred to plan time** — my standing lean is sub-sliced (isolate the try/catch runtime
  risk), the developer leans one-shot; decide once the seams are visible.
- [2026-06-22] AGREED: `throw`/`try`/`catch` use **native unwinding** (not desugar-to-Result) — the
  locked decision requires *idiomatic PHP exception* output, so the backends must reproduce real
  catch/unwind. Realistically **one new VM Op** for the handler/landing-pad stack; the interpreter
  catches at the `try` boundary (Rust `Result`). The `throws` **declaration** still erases pre-backend
  (front-end-only, no Op) — only the control flow needs the Op. Full `Op`-coupling discipline applies.
- [2026-06-22] AGREED: **Section A** — three tiers as above; `throw`/`panic*` are **`never`-typed**
  (satisfy return-on-all-paths); call-site rule = **enforce-or-propagate-or-catch**; propagation operator
  is **postfix `?`** (locked by spec lines 41/43), disambiguated from `?.`/`??` by one-char lookahead
  (propagation `?` only when not followed by `.` or `?`). Panic tier = `panic(string)`/`todo()`/
  `unreachable()`/`assert(bool, string?)`, all reusing the existing `Op::Fault`.
- [2026-06-22] AGREED: **Section B** — a thrown type is a subtype of a **core `Error` base**
  (interface/class), transpiling to a PHP class extending `\Exception` (home for `.message()` +
  cause-chain). Enforcement = enforce-or-propagate-or-catch; **declare specific** (`E-THROWS-TOO-BROAD`
  on the bare root), **catch broad**; **`main()` may not throw** (`E-UNCAUGHT-THROW`); `throws A | B`
  reuses S4 unions. `?` is type-directed: throws-call → propagate throw; `Result` value → unwrap/early-Err.

## Formal Plan
<!-- written at writing-plans time, after the spec is approved -->
