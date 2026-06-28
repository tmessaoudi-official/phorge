# M4 — Casting & conversion system (design)

> Status: **design-locked** (2026-06-26), via a research + brainstorm with the developer. Implements
> the "solid casting system" raised after `Core.List.sort`. Plan:
> `docs/plans/2026-06-26-m4-stdlib-breadth.plan.md` (Slice 2).

## The core reframe — two orthogonal axes

"Casting" conflates two operations that good designs keep separate; Phorj does too:

1. **Value conversion** — produce a *new* value of another type (`int→float`, `float→int`,
   `string→int?`). Runtime work.
2. **Type assertion / narrowing** — reinterpret an *existing* value as a more specific type (downcast
   a union/interface member; treat `T?` as `T`). No new value.

## Locked decisions (developer)

- **Assertion = a CHECKED `as` operator yielding an optional.** `v as T` ⇒ `T?` — `Some(v)` if `v` is
  really a `T` at runtime, else `None` (the Kotlin `as?` / Swift `as?` model). Composes with `??` /
  if-let. **TS's unchecked `<T>v` / `v as T` is declined** — it lies to the compiler and crashes later,
  exactly the surprise Phorj removes ([[philosophy-of-phorj]]). The checked form is the honest
  version of the developer's TS `<X>` ask.
- **No implicit coercion.** `1 + 2.0` stays a hard type error; widening is explicit
  (`Convert.toFloat(1) + 2.0`). Maximally predictable; the conversion fns make it ergonomic.
- **Surface = one system, "mixed" for free via UFCS.** Conversions live in a `Core.Convert` module;
  because UFCS ships (`x.f(a)` ≡ `f(x, a)`), `Convert.toFloat(n)` and `n.toFloat()` are the *same*
  call — module API + method API with zero duplication. The `as` operator covers assertion.

## Axis 1 — value conversion (`Core.Convert`)

Naming follows source intent (and sidesteps native overloading — the registry is keyed by
`(module, name)`, so one name = one signature):
- **`to*` = from a typed value** (total or explicitly-lossy):
  - `toString(T) -> string` — **generic, runtime-dispatched**, reusing `Value::as_display` /
    the existing `__phorj_str` PHP helper (bool→`true`/`false`, float→`__phorj_float`, else cast).
    Total. No new PHP helper (reuse `uses_str`).
  - `toFloat(int) -> float` — total widening (Rust `n as f64`; PHP `(float)`).
  - `truncate(float) -> int` — toward zero (Rust `as i64` saturating; PHP `(int)`). Lossy, **named** so
    the loss is explicit (no silent `(int)`).
  - `round(float) -> int` — half-away-from-zero (Rust `f.round() as i64`; PHP `(int)round($f)`).
- **`parse* = from a string** (fallible → `T?`)** stay in `Core.Text` (where `parseInt` already lives):
  - `parseInt(string) -> int?` — shipped.
  - `parseFloat(string) -> float?` — **add** (mirror parseInt; Rust `f64::from_str`; gated PHP helper
    matching it: reject non-numeric / surrounding ws, accept `[+-]?digits(.digits)?(e±digits)?`).
  - Rationale for the split: `parse*` signals "fallible, from text"; `to*` signals "from a typed value".
    Cross-referenced in docs. (Moving `parseInt` would churn the shipped example for no gain.)

Out-of-range `truncate`/`round` (a float beyond i64) is a documented edge (Rust saturates, PHP `(int)`
is platform-ish) — KNOWN_ISSUES; examples stay in range. All byte-identity-gated.

## Axis 2 — the checked `as` operator

`expr as Type` is a new postfix-ish operator, result type `Type?`:
- **Scrutinee**: a class / interface / union value (the same things `instanceof` accepts). `v as T`
  narrows to the member `T` when `v instanceof T`, else `None`.
- **Lowering** (front-end, no new `Op`): reuse `Op::IsInstance` — `v as T` ≡
  `if (v instanceof T) { Some(v) } else { None }` at the value level (the interpreter/VM emit the
  instanceof test + branch; result is the value or `Value::Null`). Transpiles to a PHP
  `($v instanceof T ? $v : null)`.
- **Grammar**: `as` is already a contextual word (import aliasing) — lex stays `Ident`, the parser
  recognizes `as` in expression position. Precedence: tighter than `??`, looser than member/call
  (so `a.b as T ?? d` parses as `((a.b) as T) ?? d`). Single type operand (no chains for v1).
- **Type rules**: `T` must be a class/interface/union member (else a clean diagnostic, e.g.
  `E-CAST-TYPE`); result is `T?`. Primitive `as` (e.g. `x as int` on a non-union) is rejected — use
  `Convert`/`parse*` for value conversion (keeps the two axes from blurring).
- Smart-cast interplay: `if (var t = v as T)` binds `t: T` — composes with the shipped if-let.

## Implementation slices

- **S2a — `Core.Convert` conversion natives** (additive, no language change): `toString`/`toFloat`/
  `truncate`/`round` + `Core.Text.parseFloat`. Each a registry entry; PHP via existing/gated helpers;
  guide example `examples/guide/convert.phg`. TDD kernels + 3-way byte-identity.
- **S2b — the checked `as` operator** (language change): parser + checker (`E-CAST-TYPE`, result `T?`,
  smart-cast) + interpreter/VM lowering (reuse `Op::IsInstance`) + transpiler (`instanceof ? :`) +
  `phg explain E-CAST-TYPE` + guide example `examples/guide/as-cast.phg`. No new `Op`/`Value`.

## Byte-identity notes
- `toString` reuses `__phorj_str` (already byte-identical). `toFloat`/`truncate`/`round` map to PHP
  `(float)`/`(int)`/`(int)round` — match Rust for in-range values. `parseFloat` uses a gated helper
  matching `f64::from_str` (like `parseInt`). `as` is a pure instanceof branch (run≡runvm≡PHP).
