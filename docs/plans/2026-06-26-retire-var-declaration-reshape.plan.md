# Retire `var` — declaration reshape Plan

> Breaking language change. Spec-first (developer's standing rule for breaking reshapes), then build
> fully in staged waves. Byte-identity spine (`run ≡ runvm ≡ real PHP 8.5`) is the safety net for the
> migration codemod.

## Decisions Log
- [2026-06-26] AGREED: Adopt the **synthesis declaration model** — retire the `var` keyword. A
  declaration **leads with a type** (`int x = 5`, `mutable int total = 0`); a **bare name** is
  reassignment only (`total = total + 1`). Type **inference is kept in binding positions**
  (`for (n in xs)`, if-let/while-let, `match` patterns). Net effects: no decl/assign footgun, the word
  `var` is freed as an ordinary identifier (fixes the original `$var` lift/parse bug), and the lifter
  preserves PHP names verbatim.
- [2026-06-26] Rejected: pure option 3 (types required even in bindings — heaviest, trims most
  capability); pure option 4 (bare assignment declares — reintroduces the typo footgun); Go-style `:=`
  (keeps full inference but `:=` is foreign to PHP/TS); contextual-`var` status quo (keeps the word the
  developer wants gone).

## Decisions Log (cont.)
- [2026-06-26] SUPERSEDES the "retire `var`" framing after research (Hack, Haxe, re-read
  `philosophy-of-phorge`). Corrections: (a) syntax familiarity is only *light* — craftsmanship is the
  apex filter; my `let`-false-friend argument was over-weighted; (b) tenet 4 ("never remove a good
  feature") rules OUT the keyless synthesis AND `let`=immutable (both drop the inferred-mutable
  capability + add an immutable/mutable inference asymmetry). (c) The actual bug is *a hard-reserved
  word blocking an identifier* — renaming `var`→`let` does NOT fix it (just blocks `let` instead); only
  making the keyword **contextual** fixes it.
- [2026-06-26] AGREED (final): **Keep all four declaration forms (`var`/typed × immutable/mutable) —
  additive, remove nothing. Keep the keyword spelled `var`. Make `var` CONTEXTUAL** (a legal identifier
  everywhere it denotes a *value*; the inference keyword only at declaration/binding start). The
  `mutable`/`const`/`static`/`open`/field/param model is UNCHANGED. The change is **purely additive and
  100% backward-compatible** — every currently-valid program parses identically; we only stop *rejecting*
  `var` in identifier positions. So F1 (transition) and F3 (migration) are MOOT — there is nothing to
  migrate. F2 (if-let marker) is MOOT — `if (var x = opt)` is kept verbatim.
- [2026-06-26] Verified PHP-8.5 scope rule (empirical, `php -r`): `var` is legal in PHP as a **variable,
  parameter, property, and method name**, but a **parse error as a free-function or class name**. So
  Phorge will allow `var` as a value identifier (local/param/field/property) and as a **method name**,
  and REJECT it as a free-function / class / enum / interface / type-alias name (those transpile to
  PHP symbol positions where `var` is reserved). This mirrors PHP's own tolerance exactly.

## Phase 3C convergence (30/8, per developer instruction)
- cycle 1 — RESET: F-a (`var [ … ]` at stmt-start: list-destructure decl vs index-assign on a var named
  `var`); F-b (infer-detect `var IDENT` must out-prioritize typed-decl speculation / `parse_type` must
  not swallow bare `var`); F-c (stmt-start dispatcher must split `var`-kw vs `var`-ident); F-d (all
  *value/member* identifier positions must accept `var`: primary expr, member `.var`, method call,
  param name, field name, pattern binders, catch name, method name).
- cycle 2 — RESET: F-e (the binding sites — `mutable var`, if-let, while-let, for-clause, foreach-desugar
  — must use the contextual rule); F-f (lift printer must safely emit a value named `var`, incl. as a
  decl `mutable var var = e`); F-g (impl strategy: keep `TokenKind::Var` + accept-as-ident vs drop from
  keyword map — decide in Phase 4); F-h (CLARIFY: purely additive, zero behavior change for valid
  programs); F-i (ship a guide example + KNOWN_ISSUES/CHANGELOG/CLAUDE.md doc updates).
- cycle 3 — RESET: F-k (scope: `var` is NOT a type/class name — keeps `var x = …` unambiguously the
  infer form; a type-pattern `var y` cleanly errors "`var` is not a type").
- cycle 4 — RESET: F-l (VERIFIED cross-language: PHP reserves `var` for free-function & class names →
  reject `var` there with `E-RESERVED-NAME`; allow value/param/field/property/method).
- cycle 5 — RESET: F-m (VERIFIED: no PHP-reserved-word guard exists today; `list`/`print`/`clone`/etc.
  are already usable Phorge identifiers that would transpile to invalid PHP — a PRE-EXISTING, broader
  latent hazard. Out of scope here; note in KNOWN_ISSUES as a future general reserved-name guard).
- cycle 6 — clean (1/8): re-scan — no currently-valid program changes parse (every `var`-as-keyword
  position is preserved); byte-identity spine untouched (front-end only; `var`-the-value is just a name).
- cycles 7–13 — clean (2/8 … 8/8): adversarial re-sweeps (silent byte-identity break? none — affected
  positions were compile errors before; alias `import … as var`? folds into F-d; `-e`/stdin/disasm?
  same parser; generics `List<var>`? var-not-a-type per F-k) surface nothing new.
- **CONVERGED (8/8).**

## Formal Plan

**Goal:** make `var` a contextual keyword — a legal identifier wherever it denotes a value — keeping all
four declaration forms and the whole mutability model unchanged. Additive, backward-compatible,
front-end-only, byte-identity-safe.

**Scope rule (verified against PHP 8.5):** `var` is allowed as a **local / parameter / field / property /
method** identifier (→ PHP `$var` / `->var` / `->var()`, all legal); **rejected** as a **free-function,
class, enum, interface, or type-alias** name (→ PHP reserved-word parse error) with `E-RESERVED-NAME`
(hint: "`var` is reserved in PHP here; rename"). `var` is also **not a type name** (so `var x = e` is
unambiguously the infer form).

**Disambiguation rule (statement / binding start):** the leading `var` is the inference keyword **iff**
the next token is a binding target — an identifier (`var x`), `[` (list destructure), or `IDENT {` (struct
destructure). Followed by `=` / `.` / `(` / `[`…`]` used as index / `+=` / `++` / `;` / operator → `var`
is an ordinary identifier (expression / reassignment). The `var [ … ]` corner (F-a): at statement start
`var [` is parsed as **destructure** (keyword wins); zero existing usage; to index-assign a variable
literally named `var`, parenthesize or assign via a temp (documented). The infer-detection must run
**before** typed-decl speculation (F-b).

**Steps (TDD — failing parser/checker test first at each):**
1. Parser/lexer: implement contextual `var`. Strategy decided here (F-g) — preferred: keep
   `TokenKind::Var`, add a central `ident_or_var()` accepted in all value/member identifier positions
   (F-d), and rework the statement-start + binding-site dispatch (F-c, F-e) to apply the disambiguation
   rule. Keeps the existing decl logic and avoids the `parse_type`-swallows-`var` hazard (F-b).
2. Guard: reject `var` as free-function / class / enum / interface / type-alias / type-pattern name →
   `E-RESERVED-NAME` (+ `phg explain`) (F-k, F-l).
3. Lift printer: confirm it emits a value named `var` safely, including the decl form `mutable var var`
   (F-f); add a lift round-trip case (PHP `$var` → Phorge `var`).
4. Example + docs: a guide example using `var` as a parameter/field/local (byte-identity-gated on
   run/runvm/PHP); `examples/README.md` entry; KNOWN_ISSUES note for the pre-existing general
   reserved-word hazard (F-m); CHANGELOG; CLAUDE.md (developer applies — classifier-blocked).
5. Tests: parser (contextual cases incl. F-a/F-b), checker (`E-RESERVED-NAME`), lift round-trip,
   differential (the new example). Run the full gate
   (`PHORGE_PHP=…/php-8.5.7 PHORGE_REQUIRE_PHP=1 cargo test --workspace` + clippy + fmt).

**Acceptance:** every existing test passes unchanged (additive); `var` usable as param/local/field/
property/method; rejected as fn/class/type with a clear code; `examples/**` byte-identical on all three
backends; clippy + fmt clean.

**Rollback:** single, contained surface (token.rs / lexer / parser dispatch + one checker guard) — revert
the commit; no data or migration to undo.
