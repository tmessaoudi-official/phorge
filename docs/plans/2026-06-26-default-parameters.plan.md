# Default parameters (plan)

> Language feature prerequisite for `Text.parseFloat(string, bool permissive = false)` (M4 item 6).
> Design: `docs/specs/2026-06-26-default-parameters-design.md`. Front-end fill, no backend changes,
> byte-identical run≡runvm≡PHP. Full-auto (persistent bypass).

## Decisions Log
- [2026-06-26] AGREED: `param: T = <literal>` defaults, trailing-only, literal-only, front-end fill
  (no backend change), works for functions/methods/ctors/natives. Then parseFloat uses it.

## Steps (TDD each)
1. **AST + parser** — `Param.default: Option<Expr>`; parse `= <expr>`. Thread through every `Param`
   construction site (lift, tests). Parser test: `f(int x, bool b = false)` parses with a default.
2. **Checker — signature validation** — trailing-only (`E-DEFAULT-PARAM-ORDER`), literal-only
   (`E-DEFAULT-PARAM-EXPR`), type-assignable (`E-DEFAULT-PARAM-TYPE`). Checker tests for each.
3. **Checker — call arity + fill record** — accept `[required, total]`; record (span → default exprs).
   Native path consults `native_defaults(module, name)`. Tests: under-filled call type-checks.
4. **fill_defaults pass** — apply records (append default-expr clones) in `check_and_expand` before
   backends. Backends/transpiler unchanged.
5. **Natives** — `native_defaults` lookup + `NativeDefault` enum → Expr literal.
6. **parseFloat** — `parseFloat(string, bool permissive=false) -> float?`; Rust grammar validator +
   gated `__phorj_parse_float`. Reject inf/nan; permissive adds `.5`/`5.`. Kernel tests.
7. **Example + docs** — `examples/guide/default-params.phg` (user-fn default + parseFloat), README,
   CHANGELOG, `phg explain` for the 3 new codes, KNOWN_ISSUES (first-class-value calls not filled).
8. **Gate** — full PHP-8.5 oracle workspace test; clippy + fmt; release binary; commit.

## Status
- [x] **COMPLETE** (`examples/guide/default-params.phg`, byte-identical 3-way). Front-end fill (no new
  `Op`/`Value`, no backend change) via the existing call-rewrite pass; free-functions + natives;
  `Text.parseFloat(string, bool permissive=false)` shipped as the showcase. 4 diagnostics + `phg explain`.
  6 checker tests + parseFloat kernel test + the example. **M4 item 6 unblocked + done.**
