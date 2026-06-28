# Mandatory `new` — Design

**Date:** 2026-06-24
**Status:** Design (decided with the developer; spec for review before plan/impl).
**Decision:** `new` is **required for every construction** — both class instantiation (`new Counter()`)
and enum-variant construction (`new Some(7)`, `new Circle(2.0)`). One uniform rule.

## Motivation & the accepted trade-off

The developer chose a **clean `new` everywhere** for one-rule uniformity: anything you construct with
`Name(args)` takes `new`. This was chosen over the alternative (Recommended at the time) of `new` for
classes only with bare enum variants.

- **For:** familiar to PHP/TS at the class site; visually flags object creation; matches the
  transpiler's existing PHP output (`new Counter()` / `new Circle()` — variants lower to PHP classes).
- **Accepted trade-off:** no surface language (`PHP` included) `new`s a *sum-type variant*
  (Rust `Some(7)`, Swift `.some`, PHP enum cases), so `new Some(7)` is a deliberate Phorj departure.
  The developer prizes the single rule over matching other languages' variant syntax.

Today `new` is a **reserved-but-unhandled token** (`TokenKind::New`, lexed at `lexer/mod.rs`, no parser
arm) — `new C()` is currently a parse error and `C()` is the construction syntax. This activates it.

## Surface

```
new Counter()                 // class
new Box(7)                    // generic class
new Some(7)   new Circle(2.0) new None()   // enum variants (incl. zero-arg)
```

Unaffected (literals / native calls — never take `new`): list `[1,2,3]`, map `["a"=>1]`, set
`Core.Set.of(…)`, closures `fn(x)=>…`, primitives, string interpolation.

## Mechanism (front-end only — no `Op`, no `Value`, no backend change)

`new` is a **parser-required keyword that produces the same construction AST as today**, gated by the
checker. The interpreter/VM already construct on a call to a class/variant name, and the transpiler
already emits `new` — so once the front end accepts/erases `new`, the backends are untouched and the
byte-identity spine is safe by construction.

- **Parser:** `parse_unary`/primary gains a `new` prefix: `new <call>` parses the following
  construction call and wraps it `Expr::New(Box<Expr>, Span)` (the inner is the existing
  `Expr::Call`). A bare `new` not followed by a call → parse error.
- **Checker:** a single pass over construction sites using the existing class/enum tables:
  - A `Call` whose callee resolves to a **class or enum variant** that is **not** wrapped in
    `Expr::New` → `E-NEW-REQUIRED` ("construct `Counter` with `new Counter(…)`").
  - An `Expr::New` whose inner callee is **not** a class/variant (a free function, a value) →
    `E-NEW-ON-NONCONSTRUCT` ("`new` is only for constructing a class or enum variant; call `f(…)`
    without `new`").
  - After validation the checker **unwraps** `Expr::New` to its inner `Call` (an `unwrap_new` pass,
    mirroring `expand_aliases`/`erase_generics`), so every backend sees exactly today's AST.
- **Backends:** unchanged. Transpiler still emits `new …` (it already does). Byte-identical.

## Codemod (breaking)

Every existing `Name(args)` that is a class or enum-variant construction → `new Name(args)`, across all
`.phg` + inline Rust test programs + fixtures + vendored deps. Unlike the return-type codemod, this is
**semantic** — `Counter()` and `compute()` are syntactically identical, so the codemod must know which
identifiers are classes/variants. Approach: a checker-assisted pass (load each program, consult the
class/enum tables, rewrite only construction calls) — or, for the bounded example/test set, drive it
from the known class/variant name set per file. Bare calls to free functions and natives are left
alone. `tools/new_codemod.py` (or a `phg`-internal `--rewrite-new` mode).

## Error codes (self-document via `phg explain`)

- `E-NEW-REQUIRED` — a class/variant constructed without `new`.
- `E-NEW-ON-NONCONSTRUCT` — `new` applied to a non-constructor (free function / value).

## Scope & non-goals

- No change to construction *semantics*, dispatch, or the transpile output — purely the required
  surface keyword + its enforcement.
- `new` takes no type arguments at the call (generics stay inferred: `new Box(7)`, not `new Box<int>(7)`).
- Does not interact with `const` or field initializers.

## Test plan

- Checker: `new`-less class/variant construction → `E-NEW-REQUIRED`; `new f()` (free fn) →
  `E-NEW-ON-NONCONSTRUCT`; `new Counter()` / `new Some(7)` clean.
- Parser: `new <call>` shape; bare `new` → parse error.
- Differential: every migrated example runs byte-identical `run ≡ runvm ≡ real PHP 8.5` (the codemod
  changes surface only; output is unchanged).
- `examples/guide/` — the `new` keyword is shown across the existing class/enum examples after the
  codemod; no dedicated new example needed (it's a pervasive syntax rule, like the return-type mandate).
