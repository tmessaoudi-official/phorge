# Default parameter values (design)

> Status: **design-locked** (2026-06-26). A PHP-familiar language feature, chosen by the developer as
> the prerequisite for `Text.parseFloat(string, bool permissive = false)` (M4 stdlib). Plan:
> `docs/plans/2026-06-26-default-parameters.plan.md`.

## Goal

`function f(int x, bool b = false) -> T { … }` — a trailing parameter may declare a default value, so
the argument is **optional** at the call site (`f(3)` ≡ `f(3, false)`). PHP-identical surface
(`function f($x, $b = false)`). Works for free functions, methods, constructors, **and native stdlib
functions** (so `parseFloat(s)` / `parseFloat(s, true)` works).

## The mechanism — front-end fill, ZERO backend changes

The byte-identity spine is protected by the established "expand before backends" discipline
([[type-sugar-expand-before-backends]], like `type` aliases / generic erasure / `html"…"`): a
post-check **fill pass rewrites every under-filled call to full arity** by appending the callee's
stored default expressions for the omitted trailing parameters. After the pass *every* call is
full-arity, so the interpreter, VM, and transpiler are **unchanged** — they never see a missing
argument. The default literal is the same on all three, so `run ≡ runvm ≡ PHP` is safe by construction.

Pipeline (all in the single `cli::check_and_expand` chokepoint, after name/alias resolution):
1. **Parse**: `Param` gains `default: Option<Expr>`; the parser reads `= <expr>` after `type name`.
2. **Check**: validate the signature (ordering/type/literal); on each call, allow an arg count in
   `[required, total]` and **record a fill** (span → the default exprs to append) — the same
   record-then-rewrite pattern as `html`/UFCS, since `check()` borrows the AST immutably.
3. **Fill**: a `fill_defaults` pass applies the recorded fills (append default-expr clones to the
   call's `args`).
4. **Backends/transpiler**: untouched — full-arity calls everywhere.

## Rules (checker)

- **Trailing-only** (`E-DEFAULT-PARAM-ORDER`): once a parameter has a default, every later parameter
  must too. Stricter than PHP (which deprecates a required-after-default); we reject it outright.
  `required` = the count of leading non-default params.
- **Literal default only** (`E-DEFAULT-PARAM-EXPR`): the default expression must be a literal constant
  (`int`/`float`/`bool`/`string`/`bytes`/`null`). No arbitrary/side-effecting expressions in v1 — keeps
  the inlined fill trivially byte-identical and matches PHP's constant-expression rule. (Richer
  const-fold defaults can come later.)
- **Type match** (`E-DEFAULT-PARAM-TYPE`): the default literal's type must be assignable to the param
  type (`bool b = false` ok; `int x = "no"` rejected). `null` is allowed only for an optional param.
- **Arity**: a call is valid when `args.len()` ∈ `[required, total]`; below `required` or above `total`
  stays the existing "expects N argument(s)" error.

## Natives

Native arity is a fixed `Vec<Ty>`; defaults are exceptional (only `parseFloat` needs one today). Rather
than churn ~50 `NativeFn` literals with a `default_tail: &[]`, a small lookup
`native::native_defaults(module, name) -> &'static [NativeDefault]` returns the default literals for the
**last** N params (`&[]` for every native but `parseFloat`). `NativeDefault` is a tiny literal enum
(`Bool`/`Int`/`Float`/`Str`/`Null`) converted to an `Expr` literal during the fill. The native's
`required` = `params.len() - defaults.len()`; `eval`/`php` always receive full args (post-fill), so no
native eval/transpile code changes.

## Scope (v1)

- Free functions, methods, constructors, native functions. Default applies to **direct named calls**
  (the call's callee resolves to a known function/method/native). A first-class function **value**
  called with missing args is NOT filled (`E`-level arity error as today) — closures carry no default
  metadata; a documented deferral.
- Literal defaults only (above). No `self`/param-referencing defaults (PHP forbids those too).
- Overloads: an overload set with defaults resolves by the post-fill arity (the fill is recorded per
  matched overload). v1 keeps it simple — if ambiguous, the existing overload diagnostics apply.

## Transpiler

Calls are full-arity post-fill, so the transpiler emits complete calls and standard signatures
(`function f(int $x, bool $b)`). It MAY also render the default in the signature (`bool $b = false`)
for idiomatic PHP, but since no call omits an argument it is cosmetic; v1 emits the default in the
signature for readability (it is a literal, trivially rendered) and is belt-and-suspenders byte-safe.

## Showcase

`examples/guide/default-params.phg` — a user function with a default, then `Text.parseFloat` (strict
default + opt-in permissive) as the native that motivated the feature. Byte-identical run/runvm/real PHP.

## parseFloat (the motivating native, lands with this feature)

`parseFloat(string, bool permissive = false) -> float?`:
- **strict** (default): `[+-]?digits(.digits)?([eE][+-]?digits)?` — accepts `1`, `1.5`, `-2.5e3`;
  rejects `.5`, `5.`, `inf`, `nan`, hex, surrounding whitespace.
- **permissive** (`true`): additionally accepts a lone leading/trailing dot (`.5`, `5.`).
- **Both reject `inf`/`nan`** — Rust's `f64::from_str` accepts them but PHP can't match, and the float
  rendering would diverge; rejecting keeps the spine byte-identical.
- Rust is the source of truth (a grammar validator, then `f64::from_str` on the validated slice);
  gated PHP helper `__phorj_parse_float($s, $permissive)` written to match it exactly.
