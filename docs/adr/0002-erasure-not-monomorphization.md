# ADR-0002: Generics are erased, not monomorphized

- **Status:** Accepted (2026-06-19)
- **Deciders:** project author
- **Fuller design:** `docs/specs/2026-06-17-m3-language-roadmap-design.md` — decisions **D-L2**,
  **D-L4**, **D-L9** (the transpile contract).

## Context

The transpile contract is **D-L9: Phorge is to PHP what TypeScript is to JavaScript** — every
feature transpiles to idiomatic PHP. PHP has **no native runtime generics**. Phorge plans
user-defined generics (`class Box<T>`, `function first<T>(…)`, optional bounds `T: Comparable`) at
S4.5, gated on the `Ty::Var` keystone (M10).

## Decision

Generics are **compile-time-only and erased** in the PHP output (the TypeScript model), **not
monomorphized**. Type variables are checked by the front end and then disappear from every backend's
input — interpreter, VM, and emitted PHP are all generic-free. There is **no raw `any`/`mixed`
escape hatch** (D-L2): generics + optionals + checked unions cover the dynamic-typing need. PHPStan
generic annotations may optionally be emitted as comments, but they are not load-bearing.

## Consequences

- No custom VM or runtime machinery is needed for generics; the emitted PHP stays idiomatic and
  runs under stock `php`.
- Type safety is a **front-end guarantee only** — by the time any backend sees the AST, generics are
  resolved away, so the byte-identity spine is untouched by adding them.
- A single instantiation of generic code exists at runtime (erasure), never N monomorphic copies.

## Alternatives rejected

- **Monomorphization** — there is no PHP runtime-generic target to monomorphize *into*; it would
  bloat and diverge the PHP output for zero idiomatic gain.
- **A raw `any`/`mixed` escape hatch** (D-L2) — undermines the static-typing footing; the checked
  union + optional + generic triad is the typed alternative.
