# M-RT S8 — Traits (`trait` / `use`) — Design

Status: **DESIGNED — not yet implemented.** The finale slice of M-RT (Rich Types). Adds user-facing
horizontal code reuse (`trait` + `use`) on top of the multiple-inheritance machinery built in S6.

## Context

Phorj already has explicit-resolution **multiple inheritance** (S6): `open class` parents composed via
`extends A, B` with `insteadof`/`as` conflict resolution, and S6 already lowers each MI parent to a PHP
`interface I<name>` + `trait T<name>` + a concrete class. So the *PHP backend lowering for traits already
exists end-to-end* in `transpile.rs` (`emit_decomposed_class`, `emit_multi_class`, the `as_trait` mode
that emits trait state as plain fields, the `insteadof`/`as` emission). The `trait` keyword is also already
a reserved lexer token (`lexer.rs:365`) but is otherwise unused.

This slice is therefore almost entirely **front-end semantics**: parse `trait`/`use`, flatten trait members
into using classes before any backend, and let the transpiler reconstruct native PHP `trait`/`use`.

## Decisions Log

- [2026-06-23] **D1** — S8 traits is the next slice; it closes M-RT. (Everything else in M-RT —
  totality, generic enums, overloading, S6 extends/MI, visibility modifiers — is already complete.)
- [2026-06-23] **D2** — A trait is **reuse only, NOT a type.** `use T` flattens its members into the using
  class; you cannot type a variable as `T` and `instanceof T` is rejected (`E-INSTANCEOF-TYPE`). `extends`
  = *is-a* (subtyping); `use` = *has-the-behavior-of* (reuse). This keeps traits non-overlapping with MI
  and matches PHP exactly (PHP traits are not types — the interface side is the type side).
- [2026-06-23] **D3** — Trait members carry **visibility + mutability** modifiers exactly like class
  members (a trait whose members couldn't be `private`/`mutable` would be strictly weaker than a class — an
  inconsistency surprise). This is nearly free: trait members reuse the existing `ClassMember` grammar.
- [2026-06-23] **D4** — **Maximal (Option 2):** trait constructors, `static`/`static mutable` state,
  property hooks, `const`, and abstract requirements are ALL supported. Rationale (developer, after a
  challenge round + PHP-8.4 capability evidence): "removes surprises NEVER capability" *argues for* the
  maximal set — PHP devs expect all of these; rejecting them would remove capability. Every item was
  verified to work on PHP 8.4.22 (single trait ctor; two-ctor collision resolvable via `insteadof`;
  `static mutable` per-class copy; property hooks in traits; abstract requirements; `const`; `use`+`extends`
  together).
- [2026-06-23] **D5** — Every PHP-**fatal** or PHP-**silent** trait footgun becomes a **clean
  ahead-of-time Phorj diagnostic** (the Phorj value-add over PHP — same capability, surprise converted to
  a legible error/warning *before* any backend runs).
- [2026-06-23] **D6** — P2 case (`extends Base` with a ctor + `use TraitWithCtor`, no own ctor): match PHP
  (the trait ctor wins, parent ctor is **not** auto-run) **plus** a `W-TRAIT-CTOR-PARENT-SKIPPED` warning so
  the silent "parent never constructed" surprise is visible.
- [2026-06-23] **D7** — Trait constructors ship **in this slice** (sub-slice T3); not deferred.
- [2026-06-23] **D8 (P1 treatment)** — When a class declares its own ctor AND uses a trait with a ctor, the
  class ctor wins (PHP behavior) and the trait ctor is dead unless aliased → `W-TRAIT-CTOR-SHADOWED`.
- [2026-06-23] **Version note** — This design is **PHP-version-agnostic**: every construct it emits works
  byte-identically on PHP 8.4, 8.5, and 8.6-dev (verified — full suite + oracle green on all three). PHP
  version *targeting* (raising the floor, a `--php-target` axis for 8.6-only features) is a **separate
  milestone** sequenced after S8 (developer-chosen order: name concrete 8.6 features → floor 8.5 + 8.6 in
  CI matrix → build `--php-target` → raise floor to 8.6 only once released; the last step deferred).

## Surface Syntax

```phorj
trait Loud {
    mutable int volume;                          // immutable-default + `mutable` opt-in (D3)
    const int MAX = 11;                           // trait const
    private function amp(string s) -> string {    // private trait method, flattened with visibility
        return Text.upper(s);
    }
    public function shout(string s) -> string {   // calls a sibling trait method
        return this.amp(s);
    }
    abstract function name() -> string;           // requirement the using class must satisfy
}

class Crier {
    use Loud;
    function name() -> string { return "Ada"; }   // satisfies the abstract requirement
}
```

Conflict resolution **reuses the existing S6b grammar verbatim** — users learn `insteadof`/`as` once and it
works for both `extends` parents and `use` traits:

```phorj
class C { use A, B { A.greet insteadof B; B.greet as bgreet; } }
```

## Architecture — front-end flatten, expected zero new `Op`

The byte-identity discipline (mirroring `erase_generics`, `expand_aliases`, and the S6 MI merge):

1. **Parse** — `Item::Trait(TraitDecl)` (same member grammar as a class body) and, in a class body,
   `use <Name> [, <Name>]* [{ <resolution clauses> }] ;` recorded on `ClassDecl.uses` (reusing the S6b
   `Resolution` type for the optional clauses).
2. **Checker — `flatten_traits` pass** (analogous to `merge_inherited`): for each using class, copy the
   trait's members in, applying `insteadof`/`as` resolution and abstract-requirement checking, **before any
   backend consumes the AST.** After this pass the interpreter, VM, and the rest of the checker see plain,
   complete classes — trait calls are ordinary method calls, trait fields are ordinary fields, a trait ctor
   is folded into the existing `ctor_plan`. → **Interpreter and VM need no changes** (expected; confirmed at
   T2/T3). This is the source of the *zero new `Op`* expectation — marked Inferred until implementation.
3. **Transpiler** — keeps the trait distinct (does **not** consume the flattened view) and emits **native
   PHP**: `trait T { … }` + `class C { use T; … }` + `insteadof`/`as`, reusing the S6 `emit_*`/`as_trait`
   machinery. A single-trait, conflict-free program transpiles to idiomatic hand-written PHP and is
   byte-identical to what a PHP dev would write.

The "two views" approach (backends see flattened classes; transpiler sees the trait structure) is exactly
the pattern S6 already uses for MI (`class_method_origins` for dispatch vs decomposed `interface`/`trait`
emission).

## Diagnostics (the D5 value-add)

| PHP behavior | Phorj diagnostic |
|---|---|
| Two trait ctors collide (fatal) | `E-TRAIT-CTOR-COLLISION` → require `insteadof` |
| Unresolved trait/parent method collision (fatal) | reuse existing `E-MI-*` collision error |
| Class ctor silently shadows trait ctor (P1) | `W-TRAIT-CTOR-SHADOWED` warning (D8) |
| `extends`+trait-ctor silently skips parent ctor (P2) | `W-TRAIT-CTOR-PARENT-SKIPPED` warning (D6) |
| `instanceof T` / typing a var as a trait | `E-INSTANCEOF-TYPE` (traits aren't types, D2) |
| `use` an undefined trait | `E-USE-UNKNOWN` |
| Trait abstract requirement unmet by the using class | `E-TRAIT-ABSTRACT-UNMET` |

All new codes self-document via `phg explain`. Warnings ride the existing warning channel (stderr, never
gate the build).

## Sub-slices (each a green, byte-identical commit; `run ≡ runvm ≡ real PHP`)

- **T1 — parse + method flatten + not-a-type.** `Item::Trait`, `ClassDecl.uses`, the `flatten_traits` pass
  for *methods only* (no state, no ctor), abstract-requirement check (`E-TRAIT-ABSTRACT-UNMET`),
  `E-USE-UNKNOWN`, `instanceof T` rejection (`E-INSTANCEOF-TYPE`). Transpiler emits native `trait`/`use`.
  Conflict resolution via the existing `insteadof`/`as`.
- **T2 — trait state.** Instance fields (immutable-default + `mutable`), `const`, visibility on members;
  `static` and `static mutable` (per-using-class copy — falls out of flatten-into-class).
- **T3 — trait constructors.** Fold a trait ctor into `ctor_plan`; `E-TRAIT-CTOR-COLLISION` (two unresolved
  trait ctors → require `insteadof`); `W-TRAIT-CTOR-SHADOWED` (D8); `W-TRAIT-CTOR-PARENT-SKIPPED` (D6);
  `insteadof`/alias applied to `__construct`.
- **T4 — property hooks in traits.** Flatten the synthetic `$get`/`$set` methods (M-mut.7b) like any method;
  transpiler emits the hook in the native trait.
- **T5 — example + docs + housekeeping.** `examples/guide/traits.phg` (methods + state + abstract + conflict
  resolution + a hook), `examples/README.md` row, `CHANGELOG.md`, `KNOWN_ISSUES.md` (deferrals); refresh the
  stale `CLAUDE.md` "NEXT" line; prune dead COMPLETE plan files in `docs/plans/`; close the spec + memory;
  note "M-RT CLOSED" in `docs/MILESTONES.md`.

## Deferrals → KNOWN_ISSUES

- **Traits as types** (D2) — a trait is never an `instanceof`/typing target. Use an interface for the type
  side.
- **Generic traits** (`trait T<X>`) — mirror the existing generic-method gate; out of scope this slice.
- **Compile-time ambiguity detection beyond ctors** — multi-trait method ambiguity is reported when a
  collision is unresolved (existing `E-MI-*`); broader proactive ambiguity analysis is future work.

## Acceptance

- Byte-identical `run ≡ runvm ≡ real PHP` for `examples/guide/traits.phg` (validated on the PHP-8.4 floor;
  also green on 8.5/8.6-dev).
- Full suite + clippy + fmt green on the PHP floor.
- **Expected exactly zero new `Op`** (Inferred — flatten is front-end; trait calls become normal method
  calls). If implementation reveals a genuine need for an `Op`, it extends the three coupled matches in one
  commit and the spec is updated.
- `phg explain` documents every new code.

## Rollback

Each sub-slice is an isolated commit; revert the offending commit. T1's `flatten_traits` pass + the AST
additions (`Item::Trait`, `ClassDecl.uses`) are the only broad change — if it destabilizes, `git revert`
removes the trait surface entirely (the reserved `trait` token returns to unused).
