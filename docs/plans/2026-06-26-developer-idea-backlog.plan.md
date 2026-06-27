# Developer Idea Backlog (running)

> A running log of ideas the developer pops, each with a hard-challenge verdict + recommendation +
> decision. The developer's standing process (2026-06-26): "I'll keep popping ideas till I have none —
> always include them in the roadmap, recommend actions, and discuss one-by-one via `AskUserQuestion`."
> Plan location = repo. Items move to a real milestone/slice plan once decided.

## Lens (constant)
Byte-identity Tier A (gated) vs case-by-case Tier B (impure, quarantined, fixture-tested, transpiles to
PHP). Philosophy: pragmatic, legible PHP upgrade (Phorge:PHP :: TS:JS); remove surprises, never
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
  C::class)`), so PHP allows the private construction that the global `__phorge_init_statics` would
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
