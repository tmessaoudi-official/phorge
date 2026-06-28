# Developer Idea Backlog (running)

> A running log of ideas the developer pops, each with a hard-challenge verdict + recommendation +
> decision. The developer's standing process (2026-06-26): "I'll keep popping ideas till I have none —
> always include them in the roadmap, recommend actions, and discuss one-by-one via `AskUserQuestion`."
> Plan location = repo. Items move to a real milestone/slice plan once decided.

## Lens (constant)
Byte-identity Tier A (gated) vs case-by-case Tier B (impure, quarantined, fixture-tested, transpiles to
PHP). Philosophy: pragmatic, legible PHP upgrade (Phorj:PHP :: TS:JS); remove surprises, never
capability; one obvious way.

## Batch 1 — entry-point / module model + naming (2026-06-26)

### A. `main` not always required
**State [Verified]:** only `phg run`/`runvm` require `main` (`interpreter/mod.rs:235` "no `main` function";
`compiler/program.rs:92`). `check`/`transpile`/`build` do NOT — the transpiler emits the `main()`
bootstrap only `if funcs.contains("main")` — so **library files already work without `main` today**.
**Challenge:** PHP/Python-style top-level execution (no `main`, statements run) fights the deliberate
Go/Rust explicit-entry choice (legibility; no "which file runs first" ambiguity across a package).
**Rec:** formalize "library/web files need no `main`; only running needs an entry" (clearer error,
`phg check` happy with none); KEEP explicit `main` for CLI; allow top-level ONLY for `-e`/stdin quick
scripts (a scripting affordance, not project files). **Decision: TBD.**

### B. argv/argc on `main`
**State [Verified]:** argv already available via `Core.Process.args()` (Tier B); `main` is currently
called with zero args (`interpreter/mod.rs:238`, `vec![]`). **Challenge:** (1) drop `argc` (C-ism →
use `args.length`); (2) a `main` taking argv is argv-dependent → non-deterministic → **Tier B**
(quarantined like any `Core.Process.args()` program); the no-arg `main(): void` stays pure/gated.
**Rec:** add optional `main(args: List<string>): int` (Tier B when used; also gives exit codes), keep
`Core.Process.args()` as primary, no `argc`. **Decision: TBD.**

### C. `index.phg` / web entry
**State:** M6 W1 shipped the pure `handle(Request) -> Response` value model (byte-identity-gated).
**Challenge/answer:** web entry is **not `main`** — it's `handle(Request) -> Response`; `phg serve`
(Tier B socket loop) or the transpiled PHP **front-controller** (`index.php` from superglobals) invokes
it per request. `main` ⇄ CLI, `handle` ⇄ web (parallel conventions); a web file has no `main`
(reinforces A). **Rec:** formalize `handle(Request)->Response` as the reserved web-entry convention;
serving is Tier B, the handler stays gated. (Folds into M6.) **Decision: TBD.**

### D. `len` → `length` naming consistency
**State [Verified]:** 3 words for "how many" — `List.length`, `Bytes.len`/`Text.len`, `Map.size`/
`Set.size`. **Rec (north-star JS/TS):** `length` for ordered/indexed (List, Bytes, Text) + `size` for
keyed collections (Map, Set) — exactly `Array.length`/`String.length` vs `Map.size`/`Set.size`. Rename
`Bytes.len`/`Text.len` → `.length`; keep `Map`/`Set.size`. (Alt: unify everything to `length`.) Pre-1.0
single-dev → hard rename, no alias; ~14 call sites + a codemod. Small, do-able now. **Decision: TBD.**

## Batch 2 — soundness / enforcement gaps (2026-06-26)

### E. `private`/`protected` constructor silently ignored [Verified]
External `new Secret(42)` on a `private constructor` printed `42`. Root cause: `parser/items.rs:511`
— "Modifiers preceding `constructor` are consumed and **dropped** (M1: constructors implicitly public)."
So visibility on a constructor is parsed + discarded (worse than unenforced — it *looks* like it works).
**Fix:** record constructor visibility + enforce at the `new` site (a 7th access surface beyond the six
in [[member-visibility-six-access-sites]]); only same-class / static factory may call a private ctor.
**Decision: TBD.**

### F. The wider hunt — "what other rules should we enforce?"
A "provably-correct PHP upgrade" must not accept-and-ignore a declared rule. Candidate gaps (hypotheses,
to verify): abstract-class instantiation; extending a `final` class; generic invariance at assignment
[Verified gap, KNOWN_ISSUES]; `const` local reassignment; definite-assignment of non-optional fields;
immutable-field mutation via aliases; static-vs-instance access; private-method cross-class dispatch;
interface signature variance; OTHER parsed-but-dropped modifiers (grep the `items.rs:511` smell).
**Rec:** a focused **soundness-enforcement audit** (sweep parser for dropped/ignored constructs + probe
each declared rule with a minimal program to see if it's enforced + grade severity + fix) → a findings
report feeding fix slices.
**Decision [2026-06-26]: E = FOLD into the audit (don't fix in isolation); F = RUN the soundness-enforcement
audit workflow** → findings SSOT at `docs/research/soundness-audit/SSOT.md`, fixes batched into slices after.

**Audit delivered [2026-06-27], committed `8a847d8`:** 17 rules probed → 10 enforced, **7 gaps (6 P0 + 1
P1)**, all front-end-only (byte-identity-neutral), 7 fix batches A–G. Decisions:
- **DEFER fixing — stay in design mode** (developer choice 2026-06-27). The fix queue is locked for when
  we build: **A (ctor visibility) → C (`throws` on methods) → B (generic invariance, `types.rs:228`
  reflexive short-circuit) → D (definite assignment) → F (lambda return-totality) → E (static-`this`) →
  G (dup field/param names)** — order = impact × idiomatic reach; each a green byte-identical slice + a
  guide example; autonomous one-commit-per-batch.
- **Candidates = FOLD into their parent batch** (probe-while-fixing): container-head invariance
  (List/Map/Optional/Function) with B; different-type duplicate params with G; conditional field
  assignment with D. No separate probe round.
- **Optional-field policy = DEFAULT-NULL:** an uninitialized optional field (`int? n`) reads as `null`
  (what `T?` means); non-optional fields require definite assignment (`E-FIELD-UNINITIALIZED`). Folded
  into Batch D.

## Build progress (autonomous night, 2026-06-27)
- **Batch A — constructor visibility — ✅ DONE** (autonomous). A `private`/`protected constructor`
  now blocks external `new C(...)` — the 7th member-visibility access site. Threaded `modifiers` into
  the `ClassMember::Constructor` AST node (parser no longer drops them); checker stores `ctor_vis`/
  `ctor_owner` on `ClassInfo` (inherited alongside the ctor), enforces at `check_new` via
  `enforce_ctor_vis` (`E-CTOR-VISIBILITY`), and rejects non-visibility ctor modifiers
  (`E-CTOR-MODIFIER`, closing the §5 abstract/static/… variants). A static field initializer is
  now checked in its **owning class's scope** (new `in_static_init` flag — `cur_class` set for
  visibility but `this` forbidden), so the singleton pattern is legal. **Byte-identity fix:** the
  transpiler emits the PHP visibility keyword on `__construct` AND wraps a static initializer of a
  restricted-ctor class in a class-scope-bound closure (`Closure::bind(static fn() => …, null,
  C::class)`), so PHP allows the private construction that the global `__phorj_init_statics` would
  otherwise reject — `run≡runvm≡real PHP 8.5` preserved. Example `examples/guide/ctor-visibility.phg`
  (singleton + factory-method construction) byte-identical on all three legs; `phg explain` for both
  codes; 11 new checker tests; full workspace gate green (1002 lib + 112 differential w/ PHP oracle).
  **KNOWN_ISSUE (rare, deferred):** a static init that constructs a *parent's* `protected` ctor via an
  inherited-subtype scope isn't class-scope-wrapped (the wrap keys on the field's own class having a
  restricted ctor, not an expr-walk) — needs an init-expr scan; the common self-construction singleton
  is fully covered.

- **Batch C — `throws` enforced on method calls — ✅ DONE** (autonomous). Finding #3 (P0, biggest
  idiomatic surface): a method declaring `throws E` did **not** discharge at the call site (only free
  fns did), so a checked exception escaped uncaught through the entire OO surface. Fixed by widening
  the method-overload tuple `(Vec<Ty>, Ty)` → `(Vec<Ty>, Ty, Vec<Ty>)` (params, ret, **throws**) in
  `check_method_sigs` + both `applied` builders (class + intersection arms, throws `apply_subst`-ed
  by the class θ), and discharging each matched overload's throws (single + multi-overload union),
  honoring the `?`-suppression flag — mirroring `check_overload_call`. Now `s.risky()` requires a
  `try`/`catch` exactly like `risky()`. Front-end only (no new `Op`/`Value`); `examples/guide/errors.phg`
  extended with a throwing **method** + try/catch (byte-identical run≡runvm≡real PHP 8.5); 3 new
  checker tests; full workspace gate green (1005 lib + 112 differential w/ PHP oracle). **Deferred
  (documented, narrow):** (1) method-`?` *propagation* (`x.m()?`) stays the existing `free_call_throws`
  deferral — a method throw must be caught in a `try`, not propagated; (2) an interface-method `throws`
  reached *through an interface-typed receiver* isn't discharged (the flattened iface-method form drops
  `throws`) — the concrete implementer's call still discharges, so the hole is narrow.

- **Batch B — generic type-arg invariance — ✅ DONE** (autonomous). Finding #2 (P0, the type hole):
  `Box<string>` flowed into a `Box<int>` slot (and `Option<string>` → `Option<int>`), because the
  nominal assignability arm tested the reflexive `subtype(a, a)` (always true) *before* the invariant
  arg compare — a string reached a statically-`int` slot. Fixed in `src/types.rs` `assignable_with`'s
  `(Named, Named)` arm: **split same-head (invariant per-arg compare) from a true subtype edge**
  (`if a == b { args invariant } else { subtype(a, b) }`). An un-inferred type arg defaults to
  `Ty::Error` (`new None()` ⇒ `Option<Error>`), so the per-arg compare treats `Ty::Error` as a
  wildcard (consistent with the top-level poison short-circuit) — `Option<Error> -> Option<int>` still
  binds while `Box<string> -> Box<int>` is rejected. One line closes generic classes AND generic enums.
  Container heads (`List`/`Map`/`Set`) were already invariant (they fall through `from == to`, not the
  Named arm — verified); optionals stay intentionally covariant. Pure front-end (no Op/Value change);
  3 previously-regressed inference tests (None/Ok/Result) recovered via the wildcard; 4 new generics
  tests; KNOWN_ISSUES + CLAUDE.md + `examples/guide/generic-types.phg` comment updated (gap → fixed).
  Full workspace gate green (1009 lib + 112 differential w/ PHP oracle). **Limitation (documented):** a
  *nested* un-inferred placeholder (`Box<Option<Error>> -> Box<Option<int>>`) is conservatively
  rejected (safe over-rejection) rather than bound.

- **Batch D — definite assignment + optional default-null — ✅ DONE** (autonomous). Finding #4 (P0) +
  GAP-2 (conditional assign) + optional-field policy. Two parts: (1) **checker definite-assignment** —
  `check_type_body` now runs `check_definite_assignment`: every non-optional instance field with no
  initializer and not a promoted ctor param must be assigned on **every completing path** of the
  constructor, else `E-FIELD-UNINITIALIZED` (a one-branch `if` is not every path → GAP-2 closed; no
  ctor → rejected). The path analysis (`block_assigns_field`/`stmt_assigns_field`/
  `stmt_diverges_no_return` + free `is_this_field`/`stmt_has_return` in common.rs) is the return-aware
  dual of the totality engine: a ctor `return` *completes* construction (a non-assigning path), only a
  throw/panic/infinite-loop is a saving divergence; conservative + sound. (2) **optional default-null**
  — a new front-end pass `inject_optional_field_defaults` (after `expand_aliases`) injects `= null` for
  every optional instance field lacking an initializer, so the existing field-init machinery sets it on
  all backends → `int? n;` reads `null` instead of faulting "no field n" (byte-identical, no backend
  change). 3 unsound test fixtures (declared-but-unassigned fields: GREETER, Box, C) fixed to use
  promotion. 8 new checker tests; `examples/guide/field-safety.phg` (byte-identical run≡runvm≡real PHP
  8.5); `phg explain E-FIELD-UNINITIALIZED`; README index + rejected-cases. Full workspace gate green
  (1017 lib + 112 differential w/ PHP oracle).

- **Batch F — lambda return totality — ✅ DONE** (autonomous). Finding #6 (P0, the one-line fix): a
  statement-body lambda's `LambdaBody::Block` arm never ran `check_return_totality`, so a `-> int`
  lambda that fell off the end bound `unit` into an `int` slot (free fns/methods already enforced it).
  Fixed in `check_lambda`: route the block through `check_body` (W-UNREACHABLE) + call
  `check_return_totality(&declared, stmts, span)`. Pure front-end (no Op/Value). 3 new totality tests;
  `examples/guide/totality.phg` extended with a returns-on-all-paths lambda (byte-identical run≡runvm≡
  real PHP 8.5). Reuses `E-MISSING-RETURN` (`phg explain` already covers it). Full workspace gate green
  (1020 lib + 112 differential w/ PHP oracle).

- **Batch E — static method `this`-leak — ✅ DONE** (autonomous, P0 subset). Finding #5 P0: a `static`
  method could read `this` AND bare instance fields (both checked clean + ran) — instance state with
  no instance. Fixed with a new `in_static_method` flag (the in_static_init sibling): set around a
  static method's body in `check_type_body`; `cur_class` stays set (so `Class.member` static access and
  factory ctor visibility still work), but the `Expr::This` arm rejects `this` (`E-STATIC-THIS`),
  `lookup` skips the bare-instance-field fallback in a static method, and the `Ident` arm emits a
  targeted `E-STATIC-THIS` when a bare name is an instance field. Front-end only (no Op/Value). 5 new
  checker tests; `examples/guide/static-methods.phg` (byte-identical run≡runvm≡real PHP 8.5); `phg
  explain E-STATIC-THIS`; README index + rejected-cases. Full workspace gate green (1025 lib + 112
  differential w/ PHP oracle). **Deferred (documented, ergonomic P1/P2, not unsoundness):** class-name
  static call path (`Class.method()` — calls go via an instance today), static-via-instance rejection,
  and emitting `static function` in the transpiler (a sound static method called via an instance is
  byte-identical whether or not PHP marks it `static`).

- **Batch G — declaration uniqueness — ✅ DONE; the soundness fix queue A–G is now COMPLETE**
  (autonomous). Finding #7 (P1): duplicate parameter names (free fn / method / ctor) and duplicate
  explicit field names were silently accepted (the later declaration won). **Escalation probe result
  [Verified]:** a different-type duplicate param (`f(int a, string a)`) is NOT a P0 hole — the checker
  binds the *last* type so a mismatched use is caught (run≡runvm both reject), and the runtime binds
  the last arg; consistent, so it stays P1. Fixed with a shared `reject_dup_param_names` helper
  (`(name, span)` pairs, serves `Param` + `CtorParam`) called from `collect_function`, the method arm,
  and the ctor arm → `E-DUP-PARAM`; plus an explicit-field uniqueness scan in the class collection →
  `E-DUP-FIELD`. **Kept intentional:** an explicit field that also names a promoted ctor param is
  allowed (the explicit decl is authoritative — `explicit_field_decl_wins_over_promotion`; promoted-vs-
  promoted is caught by `E-DUP-PARAM`). Front-end only (no Op/Value). 8 new checker tests; `phg explain
  E-DUP-PARAM`/`E-DUP-FIELD`; README rejected-cases (no runnable example — pure faults). Full workspace
  gate green (1033 lib + 112 differential w/ PHP oracle).

> **Soundness-audit fix queue COMPLETE (A→C→B→D→F→E→G):** all 6 P0 + the 1 P1 from
> `docs/research/soundness-audit/SSOT.md` are closed. Every fix front-end-only, byte-identity spine
> intact. Deferrals (documented in each batch above): method-`?` propagation + interface-method throws
> (C), nested generic placeholder (B), static call-path/`static function` emission (E).

### Batch 1 build progress (autonomous, 2026-06-27)
- **D `len`→`length` rename — ✅ DONE.** `Core.Bytes.len`/`Core.Text.len` renamed to `.length` (the
  native registry `name`, error messages, the `index_of` test, and all call sites: examples
  `guide/bytes.phg`/`text.phg`/`file.phg` + `web/{router,handler,server}.phg` + `tests/serve.rs`'s
  inline server program). `Map`/`Set.size` kept (JS/TS split: `length` for ordered/indexed, `size` for
  keyed). Hard rename, no alias (pre-1.0). Front-end (registry name only) → byte-identical run≡runvm≡
  real PHP 8.5; full workspace gate green. CHANGELOG's historical `Bytes.len` mention left as-is (a
  past-state record). **Next in Batch 1:** B (`main(args)`) → C (`handle()` web entry).
- **A no-main formalized — ✅ DONE.** Confirmed `check`/`transpile` already accept a library/web file
  with no `main` (the transpiler emits the `main()` bootstrap only when `main` exists). Improved the
  `run`/`runvm` "no `main`" error to a clear, consistent entry-point message naming the library path
  (`use phg check`/`transpile`). A cli test asserts a no-main library file checks + transpiles but
  run/runvm error clearly. No example (a no-main file can't be a runnable example — the differential
  requires Ok run output). Top-level statements remain unsupported in project files (Go/Rust model).

## Decisions Log
- [2026-06-26] AGREED (Batch 1):
  - **A — ADOPT:** formalize "library/web files need no `main`; only running needs an entry"; keep
    explicit `main()` for CLI; top-level statements only for `-e`/stdin quick scripts. NO PHP-style
    top-level execution in project files.
  - **B — ADOPT:** add optional `main(args: List<string>): int` (Tier B when used; exit codes), keep
    `Core.Process.args()` as primary, **no `argc`**. **`phg run <file> <args…>` passes the actual CLI
    args to `main(args)`** (the post-`--`/post-script argv, via `cli::resolve_source`'s grammar).
  - **C — ADOPT:** reserve `handle(Request) -> Response` as the web entry convention (pure, gated);
    `phg serve` (Tier B) / the transpiled PHP front-controller (`index.php`) invoke it per request.
    Folds into M6. A web file has no `main`.
  - **D — ADOPT:** `length` for ordered/indexed (List, Bytes, Text) + `size` for keyed collections
    (Map, Set), per JS/TS. Rename `Bytes.len`/`Text.len` → `.length` (hard rename, no alias; ~14 sites
    + codemod); keep `Map`/`Set.size`.
