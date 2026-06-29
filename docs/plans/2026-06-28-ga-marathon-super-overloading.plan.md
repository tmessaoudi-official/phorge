# GA Marathon — super/parent + must-use/overloading + tooling/stdlib

## Decisions Log
- [2026-06-29] AGREED (post-B2, "all of them!"): marathon continues through **all** remaining steps —
  M4 collections batch (Core.List/Map/Set/Text additive ops) → M4 numeric/string batch (Core.Math/Text)
  → step 6 cross-file LSP + JetBrains. Each additive batch ships green + byte-identical + a guide example,
  following the established native-recipe.
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
- DONE: **step 3 — return-type overloading Slice C1** (`ef108d0`): `<Type>f(args)` selector
  (`Expr::OverloadSelect`), `finalize_overloads` classification, `check_overload_select` resolution,
  per-return mangle (`rename_overload_defs` + `rewrite_ufcs` call rewrites), 4 new E-codes, guide
  example, 7 tests. 1451 green, byte-identical run≡runvm≡PHP 8.5, clippy/fmt clean. Free functions only;
  selector is the sole context (C2 widens to sinks — non-breaking, deferred).
- DONE: **step 3' — Slice C2 (partial)**: typed-binding + `return` sinks resolve a selector-less
  return-overload call from their declared type (shared `resolve_return_overload` core; `var` infer →
  NO-CONTEXT; no-assignable → AMBIGUOUS-RETURN). 4 C2 tests, example shows both. Remaining sinks
  (reassignment/field-write/arg-to-param) + `E-OVERLOAD-SELECT-CONFLICT` deferred (non-breaking).
- IN PROGRESS: **step 4 — super/parent impl** (spec `docs/specs/2026-06-28-super-parent-dispatch-design.md`).
  **Decomposition (green checkpoints):**
  - **B1 — single-inheritance `parent` (methods + ctor):** transpiles to NATIVE PHP (`parent::m()` /
    `A::m()` / `parent::__construct()`) — needs NO trait work, so it ships first as a green checkpoint.
    `Expr::ParentCall { ancestor: Option<String>, method, args, span }` (backend-visible, NOT erased);
    all three resolve the target from the shared `class_mro` + `class_method_origins` + current-class
    context (same single-source discipline as `this.m()`), so `run≡runvm`. `parent-dispatch.phg`.
  - **B2 — MI `parent(X)` (methods + ctor) + the multi-of-multi trait-lowering prerequisite:** emit any
    MI-parent and `parent(X)`-target as a PHP trait (incl. traits-using-traits), `use … { X::m as
    private __super_X_m; }` + `insteadof`; call emits `$this->__super_X_m(args)`. `parent-dispatch-mi.phg`.
  - Errors `E-PARENT-AMBIGUOUS`/`-NOT-ANCESTOR`/`-NO-METHOD`/`-OUTSIDE-METHOD`/`-NO-PARENT` (+ explain).
  Then step 5 M4 stdlib, step 6 cross-file LSP + JetBrains.

### Step 4 — concrete build (code-verified, 2026-06-29)
- **Sub-split: B1a (methods, single inh) → B1b (`parent.constructor()`) → B2 (MI + multi-of-multi trait
  lowering).** Each green + committed. `transpile/program.rs:221` routes `extends.len()>=2` →
  `emit_multi_class` (traits), else `emit_class` (native PHP `extends`) — so B1a/B1b need NO trait work.
- **Single-source resolver `ast::resolve_parent_method`** (new, in `ast/classes.rs`): given
  parents-map + `class_method_origins` + `class_mro`, lexical class, `ancestor: Option<&str>`, method →
  `Ok((decl_class, method))` or an error kind (NoParent / NotAncestor / NoMethod / Ambiguous). Immediate
  (`ancestor=None`): collect distinct origins over the direct parents → 0 NoMethod / 1 ok / ≥2 Ambiguous
  (so it already does MI). Consumed by checker (errors+typing) + interpreter (dispatch) + compiler
  (bake func index) ⇒ `run≡runvm` by construction. `parent` is **lexical** — resolves against the
  method's *declaring* class (= the `origin_class` each backend already knows at dispatch), NOT the
  receiver's runtime class.
- **AST:** `Expr::ParentCall { ancestor: Option<String>, method: String, args, span }` (backend-visible,
  NOT erased). Parser: contextual `parent` recognized only as a call head (`parent.` / `parent(`) — safe
  (no `.phg` uses `parent` as a value-ident). B1a parses `parent.<ident>(args)` + `parent(Type).<ident>(args)`;
  the `constructor` keyword arm is B1b.
- **B1a backends:** interpreter gains `cur_class: Option<String>` (lexical, set in `run_call` for a
  method body = origin class) → resolve + run the target method body with `this`=current receiver. VM:
  **one new `Op::CallParent(func_index, argc)`** (frame setup mirrors `CallMethod` but with a *baked*
  func index — non-virtual; the compiler resolves via the resolver + `methods[(decl,method)]`). Coupled
  matches: `vm/exec.rs`, `chunk.rs validate` (idx<functions.len), `compiler stack_effect` (`-(argc)`).
  Compiler emits `GetLocal(0)` (this) + args + `CallParent`. Transpiler: `parent::m(args)` (immediate) /
  `A::m(args)` (qualified ancestor) — native PHP, verified.
- **B1b (`parent.constructor()`):** parent ctor body must run on the EXISTING `this` (the VM's `<Class>::new`
  ctor functions MakeInstance a NEW instance, so they can't be reused) → decide between a front-end
  inline-the-parent-ctor-body rewrite (no backend change) vs. an interpreter run-body + VM op. Closes the
  own-ctor-under-inheritance KNOWN_ISSUE.
- Overloaded *parent* methods (resolved target is in `method_overloads`) deferred to a KNOWN_ISSUE in B1a.

### Step 4 status (2026-06-29)
- **B1a DONE + committed + green**: `parent.m(…)`/`parent(A).m(…)` single-inheritance method dispatch.
  `Expr::ParentCall` + contextual `parent` parser + `ast::resolve_parent_method` (single source) +
  interpreter `cur_class`-threaded `run_call` + VM `Op::CallParent` + native PHP `parent::`/`A::`
  transpile. Closed a 6C CTy-operand finding (compiler `ctype(ParentCall)` via `method_rets`) and the
  latent OverloadSelect arg-walking gap in the pre-`rewrite_ufcs` passes. 5 `E-PARENT-*` codes + explain;
  7 checker tests; `examples/guide/parent-dispatch.phg`. 1462 tests green, PHP-8.5 oracle byte-identical.
- **B1b DONE + committed + green**: `parent.constructor(…)` / `parent(A).constructor(…)`
  single-inheritance constructor forwarding via front-end inlining (`checker::inline_parent_ctors`, runs
  LAST in `check_and_expand`). No new `Op`/`Value`; the inlined block = param bindings + promotions +
  owner field inits + owner body (recursive for grandparents). Statement-only inside a ctor body
  (`E-PARENT-CTOR-OUTSIDE`/`-STMT`/`-MI` + shared `-NO-PARENT`/`-NOT-ANCESTOR`). Checker flags
  `in_constructor` + `parent_ctor_ok`; validates args vs `info.ctor`. Closes the own-ctor-under-inheritance
  KNOWN_ISSUE. 6 new checker tests; `examples/guide/parent-constructor.phg`; byte-identical
  run≡runvm≡real PHP 8.5.
- **B2 DONE + committed + green** (transpiler-only, as scoped): MI parent-**method** dispatch
  `parent(A).m(…)`/`parent.m(…)` now transpiles via PHP **trait aliasing** (`use … { T<dp>::m as private
  __super_<dp>_<m>; }` ⇒ `$this->__super_<dp>_<m>(…)`) in both `emit_multi_class` and decomposed-trait
  bodies; `run`/`runvm` already worked (B1a `Op::CallParent`). New transpiler field `parent_aliases` +
  `mi_parent_aliases` + a read-only `collect_parent_method_calls` walker (mirrors `rewrite_new`).
  Non-direct ancestor jump under MI → clean transpile error (PHP can't alias a transitively-used trait).
  `examples/guide/parent-dispatch-mi.phg`; 2 CLI tests; byte-identical run≡runvm≡real PHP 8.5. Deferred:
  transitive-jump-under-MI, multi-of-multi lowering, MI bare-`parent.constructor()` (per-parent
  `parent(A).constructor()` already works), overloaded parent methods.
- **NEXT: step 5 M4 stdlib breadth** (additive `Core.*` ops), then step 6 cross-file LSP + JetBrains.

### B2 — scoped (code-verified probe, 2026-06-29; built per this scope)
**B2 is a TRANSPILER-ONLY gap — much narrower than feared.** Probed with two MI programs:
- **MI parent *method* dispatch** (`parent(A).m()` / `parent(B).m()` from `class C extends A, B`):
  `run` ≡ `runvm` already produce `A+B+C` (B1a's `Op::CallParent` bakes the resolver's target; the
  resolver `ast::resolve_parent_method` already handles MI/`Ambiguous`). **Only the transpiler is
  wrong** — it emits `A::m()`, which PHP rejects (`Non-static method A::m() cannot be called
  statically`). This is THE B2 work item.
- **MI parent *constructor* forwarding** via per-parent `parent(A).constructor(1); parent(B).constructor(2);`
  **already works on all three backends** (`1/2`): B1b's inline targets each named ancestor directly and
  the transpiled inline block is plain assignments (no `parent::`). So the bare-`parent.constructor()`-under-MI
  case stays `E-PARENT-CTOR-MI` (no single target), but the idiomatic per-parent form is DONE.
**So B2 ≈ transpiler-only:** emit an MI parent-method call (`parent(X).m()`, and the qualified form that
resolves into an MI arm) via PHP **trait aliasing** — `use X { X::m as private __super_X_m; }` + the
multi-of-multi lowering (emit any MI-parent that is *also* an MI-ancestor as a trait, not just
interface+use), then the call emits `$this->__super_X_m(args)`. Existing machinery to reuse:
`transpile/program.rs` `emit_multi_class` (~2071), `insteadof_clauses` (~2116/2198), `decomposed_classes`
(`transpile/mod.rs:93`). Oracle-gated PHP-8.5 iteration — best done with fresh context.
- Implementation note (must-use): `discard` `at_discard` gate fires only on statement-leading
  `discard <Ident|new>`; `Stmt::Discard` OR-combines with `Stmt::Expr` everywhere except the checker
  (must-use exemption) and the fmt printer (emits the keyword); rewrite passes mirror Discard→Discard.

### B1b — concrete build (code-verified, 2026-06-29)
- [2026-06-29] AGREED (B1b): `parent.constructor(args)` is implemented by **front-end inlining for ALL
  backends** (run≡runvm≡PHP get the identical inlined AST ⇒ byte-identity safe by construction). The
  inlined block (a `Stmt::Block`, new lexical scope) mirrors one `construct` plan entry for the resolved
  parent: ① param bindings `Ty p = arg;` (verified: a let-init reads the OUTER scope, so same-name
  forwarding `parent.constructor(x)` with parent param `x` is correct), ② promotions `this.p = p;` for
  vis-modifier params, ③ the parent's OWN field initializers `this.f = init;` (from
  `ast::field_initializers(prog, target)` — PHP-faithful: only the invoked-ctor class's own inits),
  ④ the parent ctor body (from `ast::ctor_plan(prog, target)`, recursively inlined for grandparents with
  the target as the new lexical class). NO new `Op`/`Value`.
- **Scope: single inheritance only** (MI ctor forwarding = B2). Immediate `parent.constructor()` →
  direct parent (≥2 parents → `E-PARENT-CTOR-MI`, deferred); qualified `parent(A).constructor()` → A
  must be a transitive ancestor. Validation reuses `info.ctor` (effective param types) via `check_args`;
  arity covers the ctor-less-parent case (zero-arg forward = no-op inline).
- **Position-restricted:** `parent.constructor(…)` is allowed ONLY as a bare statement inside a
  constructor body (new checker flags `in_constructor` + `parent_ctor_ok`) → guarantees the inline pass
  catches every occurrence and the backends never see a `ParentCall{constructor}`. New codes
  `E-PARENT-CTOR-OUTSIDE` (not in a ctor body), `E-PARENT-CTOR-STMT` (used as a value), `E-PARENT-CTOR-MI`.
- **Pipeline:** `inline_parent_ctors` runs LAST in `cli::check_and_expand` (after `rename_overload_defs`)
  so the cloned parent body is already fully de-sugared. Closes the own-ctor-under-inheritance KNOWN_ISSUE.

## Slice C (return-type overloading) — implementation approach (discovered, NOT yet built)
**Key finding:** `Checker::check_expr(&mut self, expr) -> Ty` is purely bottom-up — no expected-type
param — and `E-OVERLOAD-RETURN` is enforced at `checker/collect.rs:1199`. So Slice C is **interlocking**
(relax-invariant + resolve + backend-dispatch must land together; runtime can't pick among same-param
different-return overloads) and **has no small green checkpoint**. Recommended decomposition:
- **C1 (minimal-first, per spec):** explicit `<type>f(...)` selector ONLY (expected type is *local* at
  the call → no bidirectional change). Relax `E-OVERLOAD-RETURN` to allow differing returns; a
  return-overloaded call WITHOUT a selector → `E-OVERLOAD-NO-CONTEXT`; same-return sets keep working
  selector-free. Resolve the member at the selector; **mangle each return-overload member to a distinct
  name + rewrite resolved call sites** (reuse the cross-package mangle/rewrite discipline + `erase_generics`
  "rewrite-before-backends" pattern — NO new Op/Value, NO runtime-dispatch change). Transpiler emits the
  mangled names (single-return names stay bare → existing programs byte-identical). Parser: `<Type>` prefix
  production at operand position (`<` is infix-only today, so it's free) + the `>>` nested-generic split
  (reuse the type-annotation parser's split).
- **C2 (widening, non-breaking):** add bidirectional expected-type propagation at the 5 shallow sinks
  (typed binding / typed reassignment / typed field write / `return` / non-overloaded typed param) so the
  selector becomes optional there. This threads an `Option<&Ty>` expected type into `check_expr` (or a
  dedicated `check_expr_expected`) at those sites only.
- Errors: `E-OVERLOAD-NO-CONTEXT`, `E-OVERLOAD-AMBIGUOUS-RETURN` (exact→unique-assignable→else),
  `E-OVERLOAD-SELECT-UNKNOWN`, `E-OVERLOAD-SELECT-CONFLICT`. Resolution rule + PHP mangling per the spec.
- **No small green checkpoint → implement in one focused (ideally fresh-context) session.**

## Slice C1 — concrete build (this session, 2026-06-28)
Code-verified refinements to the spec (from mapping `dispatch.rs`, `checker/{collect,calls,expr,mod}.rs`,
`rewrite_ufcs.rs`, `cli/mod.rs check_and_expand`, parser `exprs.rs`/`types.rs`, lexer):
- **DECISION (scope): C1 is FREE FUNCTIONS ONLY.** Return-overloaded *methods* stay rejected
  (`E-OVERLOAD-RETURN`/`-DUPLICATE` unchanged on the method collection path). The `<T>` selector is
  therefore only meaningful on a free-function call; a selector on anything else → `E-OVERLOAD-SELECT-UNKNOWN`.
  Methods deferred to a follow-up (KNOWN_ISSUES).
- **DECISION (set shape): a return-overload set = ALL members share identical parameter signatures
  (one param-group) with ≥2 distinct return types.** Mixed (≥2 param-groups with a differing return)
  stays `E-OVERLOAD-RETURN` (repurposed message: "cannot mix parameter- and return-overloading").
  Rationale: identical params ⇒ no param-matching ambiguity ⇒ statically decidable; avoids the subtyping
  ambiguity that motivated E-OVERLOAD-RETURN, and avoids feeding the backends an ambiguous
  identical-`ParamKind` dispatch table.
- **No `>>` trap:** lexer emits single `Gt` for `>`, two `Gt` for `>>` (lexer:1112/1154), so
  `<List<int>>f()` parses with no split. Selector parse = `Lt` → `parse_type()` → `expect(Gt)`.
- **AST:** new `Expr::OverloadSelect { ty: Type, call: Box<Expr>, span }`. Parsed at prefix position in
  `parse_unary` (a leading `Lt` cannot begin an operand today → unambiguous). Erased before backends
  (like `Expr::New`/`html`): exhaustive Expr matches in the true backends get `unreachable!` arms; `fmt`
  printer + `ast::walk` + lift printer get real arms (`<Ty>` + recurse).
- **Checker classification (finalize between `collect` and `check_program`):** new
  `finalize_overloads()` reads `self.funcs`, and for each free-fn name that is a return-overload set
  records `self.return_overload_sets: HashMap<String, Vec<(Ty ret, String mangled)>>` (consumed by call
  checking) and `self.overload_def_renames: HashMap<usize span.start, String mangled>` (consumed by the
  rename pass). Decl spans come from a parallel `self.free_fn_decls: Vec<(name, Span, params, ret)>`
  accumulated in collection. Mangle = `{name}__ret_{slug(ret)}` (non-alnum→`_`).
- **Validation (`validate_new_overload`, free-fn path only via a new `allow_return_overload` flag):**
  same-params+same-ret → `E-OVERLOAD-DUPLICATE`; same-params+diff-ret → ALLOW (forms/extends R-set);
  diff-params+diff-ret on a singleton → `E-OVERLOAD-RETURN` (classic, kept); any diff-params member added
  to an R-set, or any same-params member added to a P-set → `E-OVERLOAD-RETURN` (mixed). Keep
  STATIC-MIX/GENERIC/ERASE.
- **Selector resolution (`check_overload_select` from the `OverloadSelect` arm of `check_expr`):**
  resolve `ty`→`Ty`; require callee = free-fn return-overload set (else SELECT-UNKNOWN); type the args +
  arity-check against the shared param sig (E-OVERLOAD-NO-MATCH); pick member: exact `ret==T` →
  unique `ret<:T` → else 0 SELECT-UNKNOWN / ≥2 AMBIGUOUS-RETURN; record
  `overload_resolutions[select_span.start] = Call{ Ident(mangled), args, call_span }`; discharge the
  chosen member's throws; return its ret. Bare return-overload call (no selector) reaches
  `check_named_call` → `E-OVERLOAD-NO-CONTEXT` (C2 sinks resolve these later).
- **Wiring:** `check_resolutions` merges `overload_resolutions` into `calls` (applied by `rewrite_ufcs`,
  whose `rexpr` gains an `Expr::OverloadSelect` arm that looks up `span.start`) AND returns
  `overload_def_renames`; `check_and_expand` runs a new `rename_overload_defs(program, &renames)` pass
  (renames `FunctionDecl.name` by span) before the existing chain. Single-return names stay bare ⇒
  existing programs byte-identical.
- **Errors:** `E-OVERLOAD-NO-CONTEXT`, `E-OVERLOAD-AMBIGUOUS-RETURN`, `E-OVERLOAD-SELECT-UNKNOWN`,
  `E-OVERLOAD-SELECT-CONFLICT` (C2-only; conflict needs a sink) — all `phg explain`-documented.
  `E-OVERLOAD-RETURN` repurposed (no longer "must share return"; now "different params AND returns" /
  "mixed param+return").
- **Example:** `examples/guide/return-overloading.phg` (selector-resolved calls + `discard <T>f()`).
  Byte-identical run≡runvm≡real PHP 8.5; the mangled names round-trip.
- **C2 (deferred):** the 5 shallow sinks (typed binding/reassign/field-write/return/non-overloaded
  typed param) thread an `Option<&Ty>` expected type so the selector becomes optional there.
