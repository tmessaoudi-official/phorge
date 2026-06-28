# GA Marathon — super/parent + must-use/overloading + tooling/stdlib

## Decisions Log
- [2026-06-28] AGREED: marathon order (user-confirmed) —
  1. **Spec super/parent dispatch** (design + doc)
  2. **must-use returns (Slice A)** + breaking codemod
  3. **return-type overloading (Slice C)** — depends on must-use
  4. **super/parent dispatch impl (Slice B)**
  5. **M4 stdlib breadth** (additive Core.* ops)
  6. **cross-file LSP + JetBrains plugin** (last — tracks the now-stable grammar)
  Autonomous; each slice green + byte-identical + committed; stop only on a genuine design fork; developer pushes.
- [2026-06-28] AGREED (must-use): any non-`void`/`Empty` expression-statement must be used or
  `discard`-ed (scope Option 1). `discard <expr>` is a contextual keyword (not `void f()`). Breaking → codemod.
- [2026-06-28] AGREED (return-type overloading): overloads may differ only in return type; resolved
  compile-time from a SHALLOW/direct-only sink set (typed binding / typed reassignment / typed field
  write / `return` / non-overloaded typed param); everywhere else needs `<type>f(...)`.
- [2026-06-28] AGREED: `<type>f(...)` is an overload SELECTOR, distinct from `as` (cast). Subtyping
  resolution = exact → unique-assignable → else `E-OVERLOAD-AMBIGUOUS-RETURN`. Sink/selector
  disagreement → `E-OVERLOAD-SELECT-CONFLICT`.
- [2026-06-28] CONCEDED to user: `discard <int>f()` is valid (compiler can't enforce side-effect
  parallelism); bare `discard f()` on a return-overload → `E-OVERLOAD-NO-CONTEXT` (missing selector).
- [2026-06-28] AGREED: PHP transpile of return-overloads via per-return name-mangling; single-return
  names stay bare (existing programs byte-identical). Param-overloads stay runtime-dispatched.

## Specs
- `docs/specs/2026-06-28-must-use-and-return-type-overloading-design.md` — written, user-approved (syntax-fixed).
- `docs/specs/2026-06-28-super-parent-dispatch-design.md` — written; pending review.

## super/parent decisions (locked this session)
- Syntax: `parent.m()` (immediate) / `parent(A).m()` (qualified, call-style). Contextual keyword.
- Chaining: **explicit only** (no auto-chain); `parent(A)` may name **any** transitive ancestor
  (C++-style jump allowed). Bare `parent.m()` ambiguous in MI → `E-PARENT-AMBIGUOUS`.
- Methods **and** constructors; MI ctor = `parent(P).constructor(args)` per parent. Fields out of scope.
- PHP emission VERIFIED against real 8.5: single-inh native (`parent::`/`A::m()`/`parent::__construct`),
  MI via trait `use`+`insteadof`+`as` aliasing. **Prerequisite:** complete the multi-of-multi trait
  lowering (currently a KNOWN_ISSUE) first/with the feature.
- Error codes: `E-PARENT-AMBIGUOUS`, `E-PARENT-NOT-ANCESTOR`, `E-PARENT-NO-METHOD`,
  `E-PARENT-OUTSIDE-METHOD`, `E-PARENT-NO-PARENT`.
- Stale-syntax lesson: Phorj returns are `: Type` (not `-> Type`); typed local is `T x =` (no `var T x`);
  function-types use `=>`.

## Status
- DONE: M-perf S1b slot-indexed fields (`6b71232`) + S2 VM inline cache (`7152edf`, ~20% field-heavy).
- DONE: both design specs (`ef086bb` must-use/overloading, `9c6e27e` super/parent).
- DONE: **step 2 — must-use Slice A** (`53fa3af`): `Stmt::Discard` + contextual `discard` keyword;
  E-UNUSED-VALUE on non-{void,Empty,never,Error} expression-statements; front-end-only (run≡runvm≡PHP);
  codemod (mutable-fields, static-fields + 3 inline tests); guide example + explain. 1444 tests green.
- NEXT: **step 3 — return-type overloading (Slice C)**, then step 4 super/parent impl, step 5 M4 stdlib,
  step 6 cross-file LSP + JetBrains.
- Implementation note (must-use): `discard` `at_discard` gate fires only on statement-leading
  `discard <Ident|new>`; `Stmt::Discard` OR-combines with `Stmt::Expr` everywhere except the checker
  (must-use exemption) and the fmt printer (emits the keyword); rewrite passes mirror Discard→Discard.
