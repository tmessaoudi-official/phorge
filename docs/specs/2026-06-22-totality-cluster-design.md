# Totality Cluster — Design (M-RT, pre-overloading)

> **Status:** design-locked (roadmap-completeness audit, 2026-06-22). The #1 soundness leak in the
> type system: a `-> T` function may today fall off the end and silently "return" nothing. This slice
> closes it and ships the supporting bottom type + two dead-code lints. All four sub-features are
> **front-end-only** (checker + one PHP type-hint + one `resolve_type` arm): **no new `Op`, no `Value`
> change, byte-identity-safe by construction** (`run ≡ runvm ≡ real PHP`).

## The four sub-features

1. **Return-on-all-paths (`E-MISSING-RETURN`)** — a function whose declared return type is non-`unit`
   must return (or diverge) on *every* path. The headline fix.
2. **`never` type (`Ty::Never`)** — the bottom type: subtype of every `T`, inhabited by nothing.
   A `-> never` function must provably diverge (`E-NEVER-RETURN` otherwise). Transpiles to PHP 8.1
   native `never`. Today's only divergence producers are infinite loops and calls to other `never`
   functions; when the error model (M-faults Slice 2) lands, `throw` becomes another diverging form and
   `never` "lights up" fully — this slice wires it correctly ahead of that.
3. **`W-UNREACHABLE`** — a non-fatal lint: a statement that follows a diverging statement
   (`return`, infinite loop, `never`-call) in the same block is dead code.
4. **`W-MATCH-UNREACHABLE`** — a non-fatal lint: a `match` arm after a catch-all (`_`/bare binding),
   or a duplicate literal/variant/type arm, can never be reached.

`E-*` are hard errors (gate the build); `W-*` ride the existing warning channel (`check()` returns
`Ok(warnings)`, rendered to stderr, never fail the build) — the `W-FORCE-UNWRAP` precedent.

## The shared engine: structural termination analysis

One pure, side-effect-free checker method answers *"does this statement / block definitely transfer
control away and never fall through?"* It is the engine behind sub-features 1 and 3.

```text
block_terminates(stmts)  :=  any statement in stmts terminates
                             (once one diverges, the rest are dead; the block as a whole diverges)

stmt_terminates(s) := match s
  Return                                 => true
  Block(b)                               => block_terminates(b)
  If { then, else: Some(eb) }            => block_terminates(then) && block_terminates(eb)
  If { else: None }                      => false                  (the false path falls through)
  While  { cond, body, post_cond:false } => is_true_lit(cond)  && !breaks_this_loop(body)
  While  { cond, body, post_cond:true  } => block_terminates(body) || (is_true_lit(cond) && !breaks_this_loop(body))
  CFor   { cond, body }                  => cond.is_none_or(is_true_lit) && !breaks_this_loop(body)
  Expr(e)                                => expr_is_never(e)
  _                                      => false
```

**Soundness direction.** `terminates` must never claim divergence it cannot prove — a *false* `true`
would suppress a real `E-MISSING-RETURN`. It is therefore deliberately conservative: it returns `true`
only for shapes that demonstrably never fall through. A *false* `false` (failing to recognise a real
divergence) only costs an over-strict `E-MISSING-RETURN`, which the realistic-shape coverage above is
sized to avoid (the Phase-0 example/test scan validates this empirically).

**`breaks_this_loop(stmts)`** — recursively scans for a `break` bound to *this* loop: descends into
`If`/`Block` but **not** into nested `While`/`CFor`/`For` (their breaks bind to them). `match` arms are
expressions and carry no `break`.

**`expr_is_never(e)`** — immutable, error-free; recognises a `never`-typed expression *without*
re-checking: a call to a free function whose stored signature returns `Ty::Never`, and (for
composability) a `match`/`if` expression all of whose arms are themselves `never`. Method-returning-
`never` is deferred (needs receiver typing) — documented, not blocking.

## Integration points (exhaustive)

- **`src/types.rs`** — `Ty::Never` variant; `Display` ⇒ `"never"`; two `assignable_with` arms:
  `(Ty::Never, _) => true` (bottom: flows into any slot) placed **before** the `Null` arms so it wins,
  and `(_, Ty::Never) => from == Ty::Never` (nothing else is assignable *to* never) via the final
  `from == to`. The `(Ty::Never, _)` arm must precede `(Ty::Null, _) => false` so `never → T?` succeeds.
- **`src/checker.rs`**
  - `resolve_type`: `"never" => self.no_args(name, args, *span, Ty::Never)` alongside the primitives.
  - `is_builtin_type_name`: add `"never"` (reserve it — no user type may shadow it).
  - `check_function`: after walking the body, gate on the resolved return type — `Unit`/`Error` exempt,
    `Never` requires `block_terminates(&f.body)` else `E-NEVER-RETURN`, anything else requires it else
    `E-MISSING-RETURN`. `return expr;` / `return;` inside a `-> never` function already fail through the
    existing `err_assign` (only `never` is assignable to `never`) — no extra code.
  - New `check_body(stmts)`: the unreachable-scan + per-statement check loop (no scope push). Replaces
    the three open-coded `for s in body { check_stmt }` loops (free fn, constructor, `set` hook).
    `check_block` becomes `push_scope → check_body → pop_scope`, preserving today's scoping exactly.
  - `check_match`: extend the existing arm loop with `W-MATCH-UNREACHABLE` (after-catch-all + duplicate
    literal/variant/type detection). Pure addition; exhaustiveness logic untouched.
  - `stmt_span` helper (mirrors `expr_span`) for the `W-UNREACHABLE` diagnostic location.
- **`src/transpile.rs`** — `emit_type`: `"never" => "never".into()` (else it falls to `php_type_ref`
  and is mis-emitted as a class FQN). Return-only in practice; never-typed params can't be called.
- **`src/cli.rs`** — `explain_text`: paragraphs for `E-MISSING-RETURN`, `E-NEVER-RETURN`,
  `W-UNREACHABLE`, `W-MATCH-UNREACHABLE`.
- **No compiler (`CTy`) change** — `resolve_cty` keys on type name; `"never"` falls to `CTy::Other`
  (a `never` value never exists, so it is never an arithmetic operand). Verified, not assumed.

## Byte-identity argument

Nothing reaches the VM/interpreter that did not before: `never` is erased to a PHP return hint and is
otherwise checker-only; `E-MISSING-RETURN`/`E-NEVER-RETURN` reject programs *before* any backend runs;
the two `W-*` lints emit to stderr and change neither stdout nor the AST. A `-> never` PHP function
never returns at runtime (it diverges), so the `: never` hint matches semantics. The differential spine
(`run ≡ runvm ≡ real PHP`) is safe by construction.

## Phase-0 latent-bug scan

Turning on `E-MISSING-RETURN` may surface existing typed functions that fall off the end — in shipped
examples or the inline test programs. Each is either a real latent bug (fix it: add the return) or a
`terminates` coverage gap (fix the engine). The full gate (`PHORJ_REQUIRE_PHP=1 cargo test` +
`all_examples_match_between_backends`) is the scan; every failure is triaged in-slice.

## Example

`examples/guide/totality.phg` — a `classify(int) -> string` returning on all `if`/`else` paths, a
`pick(bool, int, int) -> int` returning via both arms of an expression `if`, and a `loopForever()
-> never` infinite loop referenced (not called) to exercise the bottom type — byte-identical on
`run`/`runvm`/real PHP, plus a `README` note on the `E-MISSING-RETURN` it prevents (a fault can't be a
runnable example).

## Deferred (KNOWN_ISSUES)

- Method/closure calls returning `never` in `expr_is_never` (needs receiver typing).
- `never` as a usefully-inhabited type awaits `throw`/`panic` (M-faults Slice 2).
- Flow-typing beyond structural termination (e.g. exhaustive-`match`-statement-as-divergence).
