# Visibility modifiers (`public` / `internal` / `private`) — Plan

> Top-level declaration visibility for Phorge: a three-level lattice on every top-level item
> (class, enum, interface, function). Successor to the closed mutation milestone (M-mut.7b).

## Decisions Log
- [2026-06-21] AGREED: **fork chosen = class-visibility feature** (over M-RT overloading / PHP-parity review). Autonomy directive `f5f2912` was scoped to the now-closed mutation milestone; this is a fresh design pass.
- [2026-06-21] AGREED: **scope = ALL top-level declarations** — `class`, `enum`, `interface`, AND free `function` (not classes-only as originally locked). Rationale: the loader's `defined` (functions) + `types` (classes/enums/interfaces) tables share one resolution chokepoint, so covering all four ≈ same effort as classes-only; a file-private *helper function* is the most common real use.
- [2026-06-21] AGREED (challenge accepted): **three-level model — `public` default, `private` = file-scoped, `internal` = package-scoped.** Lattice `file ⊂ package ⊂ public`. Default (no keyword) = `public` (cross-package). Challenged against Go's two-level package-only model; the developer chose file-default-private deliberately — consistent with Phorge's established strictest-sensible-default tradition ("nothing in the wind" namespaces, explicit-import-even-for-stdlib). Cost (same-package files can't share a `private` helper) is mitigated by `internal` + a targeted diagnostic.
- [2026-06-21] AGREED: **package-level keyword = `internal`** (over `package` — collides with the `package Main;` header — and `shared`). Established term (C#/Kotlin/Swift/D).
- [2026-06-21] VERIFIED (not a change): the `package` keyword is **kept** (reshape design D1); we did NOT rename it to `namespace`. PHP *output* uses `namespace A\B {}`; Phorge *source* keyword stays `package` (TS:JS-style lowering).
- [2026-06-21] AGREED: **execution = fully autonomous** — build all 8 impl tasks straight through, no per-task checkpoint, stop only on a genuine craftsmanship fork or a red quality gate (mirrors mutation directive `f5f2912`). `_AUTONOMOUS_3C=1` for this feature.
- [2026-06-21] AGREED: **design approved** ("Approve — write the spec"). Locked: loader-enforced; visibility never consumed by backends (byte-identity safe by construction, no erase pass); new `Visibility { Public, Internal, Private }` enum field on `ClassDecl`/`EnumDecl`/`InterfaceDecl`/`FunctionDecl` (NOT overloading member `Modifier`); explicit `public` keyword **allowed** (intent-clarity, not dropped); `internal` is a new reserved keyword; codes `E-VIS-PRIVATE`/`E-VIS-INTERNAL`. Single-file/loose mode = no-op. Spec: `docs/specs/2026-06-21-visibility-modifiers-design.md`.

## Formal Plan
The bite-sized, TDD, task-by-task implementation plan lives in
**`docs/plans/2026-06-21-visibility-modifiers-impl.plan.md`** (8 tasks: AST enum/fields → parser
keyword → loader provenance + cross-package type enforcement → file-scoped private type → function
visibility → explain + alias guard → example project → docs sweep). Each task ends green on the full
`PHORGE_REQUIRE_PHP=1` gate.

## STATUS

**COMPLETE (2026-06-21)** — feature shipped, fully autonomous build. Commits:
`d79027c` (AST enum/fields) · `d0ce40f` (parser keyword) · `4562afb` (loader enforcement — Tasks 3-5
combined, since they share `ResolveCtx` and can't pass `-D warnings` independently) · `b60821c`
(explain + alias guard) · `dcbcb9c` (example project) · this commit (docs sweep). 682 tests green,
clippy + fmt clean, `examples/project/visibility/` byte-identical `run ≡ runvm ≡ real PHP`. Deferred
items recorded in KNOWN_ISSUES. Plan files may be removed now that the feature is closed.
