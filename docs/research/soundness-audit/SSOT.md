# Soundness-Enforcement Audit — SSOT

> **STATUS [2026-06-27]: ALL FIXES SHIPPED.** The fix queue A→C→B→D→F→E→G is complete — all 6 P0 + the
> 1 P1 closed, each a green byte-identical front-end-only commit (A `4cf4939`, C `b3ee332`, B `489adfc`,
> D `a198f38`, F `7c7e488`, E `dafcd01`, G `e16f289`). Per-batch details + documented deferrals in
> `docs/plans/2026-06-26-developer-idea-backlog.plan.md` (Build progress section).


**Date:** 2026-06-27 · **Binary probed:** `/stack/projects/phorj/target/release/phg` (prebuilt
release, never rebuilt) · **Method:** one minimal probe program per declared rule, run through
`phg check` / `phg run` / `phg runvm` / `phg transpile`; a rule is a GAP only when a program that
*should* be rejected instead checks/runs cleanly (or a modifier has no effect). All verdicts below
are backed by pasted command output in `docs/research/soundness-audit/raw/`.

---

## 1. Executive summary

For a language whose pitch is a **provably-correct upgrade of PHP**, a "declared but not enforced"
rule is the worst defect class — it *looks* like it works. This audit probed **17 declared
soundness rules**.

- **10 rules are genuinely ENFORCED** (rejected for the right reason, positive control passes).
- **7 rules are GAPS** — **6 are P0 (unsound: provably-wrong code passes the checker)**, 1 is P1
  (a declared uniqueness rule silently ignored).
- Discovery surfaced **5 further modifier-drop variants** (abstract/static/const/open/public
  constructor; contradictory member modifiers; meaningless method modifiers; unassigned-optional
  field faults-not-null) that were not individually Stage-2 probed but are evidenced in the
  discovery raws — flagged in §5 for a follow-up probe.

**Headline risk:** the type system has **multiple live type holes**. The single worst is **generic
type-argument invariance** (a `Box<string>` flows into a `Box<int>` slot, then arithmetic on the
string faults at runtime) — a string reaches a statically-guaranteed `int`. Equally severe and
*more pervasive in idiomatic code*: **`throws E` is not enforced on method calls** (only free-fn
calls), so the headline "fix to PHP's unenforced `@throws`" is silently bypassed for the entire OO
surface — the way most real Phorj code raises errors. The **seed bug** (private/protected
constructor parsed and dropped) is confirmed and is finding #1 (§4): construction is the missing
7th member-visibility access site.

**Cross-check note:** every Stage-2 verdict was re-checked against its own pasted output. All
conclusions are supported by their evidence — **no false positive/negative to drop or downgrade**.
One clarification: the `probe-static-instance` roll-up summary says "one P0"; its detail correctly
identifies Finding 1 (a `static` method reading `this`) as the P0, with secondary P1/P2 findings —
consistent, retained.

---

## 2. CONFIRMED GAPS (severity-ranked)

| # | Sev | Rule | What's wrong | Evidence excerpt | Fix (file / check) |
|---|-----|------|--------------|------------------|--------------------|
| 1 | **P0** | `private`/`protected constructor` must block external `new C(...)` | Modifier parsed then **dropped at parse time**; AST node has no visibility field. External `new` checks + runs on all 3 backends. | `phg check private-ctor.phg → OK`; `phg run → 42`; `runvm → 42` (`probe-private-ctor.md`). `protected` identical → `7` (`probe-protected-ctor.md`) | `src/ast/mod.rs` add `visibility` to `ClassMember::Constructor`; `src/parser/items.rs:516` stop dropping `modifiers`; `src/checker/calls.rs` reject private/protected ctor outside class/hierarchy via `enforce_member_vis` → new `E-CTOR-VISIBILITY` + `phg explain` |
| 2 | **P0** | Generic type args invariant at an assignment boundary (`Box<string>` ✗→ `Box<int>`) | `subtype(a,b)` is evaluated first in the `||`; the oracle `is_subtype` is reflexive (`a==b ⇒ true`), so same-head short-circuits **before** the `aa==ba` invariant arg compare → invariant check is dead code. | `phg check generic-invariance.phg → OK`; probe 2 `int x=b.get(); x+1` → `phg run` faults `cannot apply Add to string and int` (`probe-generic-invariance.md`) | `src/types.rs:228` — split same-head: `if a==b { aa==ba } else { subtype(a,b) }`. One fix closes generic classes **and** generic enums (`Option<string>`→`Option<int>`, same line) |
| 3 | **P0** | `throws E` must be enforced at the **call site** (unhandled → `E-CALL-UNHANDLED`) | Free-fn calls discharge `throws`; **method calls do not** — the overload tuple `applied: (Vec<Ty>,Ty)` drops the `throws` set before `check_method_sigs`, so it's structurally absent. Checked exception escapes to `main` uncaught. | `phg check method-throw.phg → OK`; `phg run` → `uncaught exception A`. Identical body as a free fn → `E-CALL-UNHANDLED` (`probe-uncaught-throws.md`) | `src/checker/calls.rs` — widen method-overload tuple to `(Vec<Ty>,Ty,Vec<Ty>)`; in `check_method_sigs` discharge each matched overload's `throws` (mirror `check_overload_call:213-223`); wire method-call `?` into `try_throws_propagate`; tests in `src/checker/tests/throws.rs` |
| 4 | **P0** | Every non-optional instance field must be definitely initialized | Non-optional field with no initializer + never assigned: `check` clean, runtime faults `no field x` — the type system's `T` promise is unbacked. (A bare non-`mutable` field can't even be ctor-assigned → permanently latent.) Asymmetry: **static** fields *do* require init (`E-STATIC-NO-INIT`). | `phg check definite-assign.phg → OK`; `phg run` → `runtime error: no field x on Secret`. run≡runvm both fault → invisible to differential (`probe-definite-assign.md`) | `src/checker/program.rs::check_type_body` — definite-assignment pass: required = non-`Optional` fields w/o initializer; walk ctor body for `this.f=` on all paths (reuse totality `stmt_terminates` join); new `E-FIELD-UNINITIALIZED`. Front-end only |
| 5 | **P0** | A `static` method must not access `this`/instance state | `static` is parsed + retained but `FnSig` carries no static flag and `check_type_body` checks static methods with `cur_class` set → `this` in scope inside a static body; checks clean + runs. Transpiler **drops** `static`, hiding the divergence (PHP would fatal on `$this`). | `phg check static-uses-this.phg → OK`; `phg run → 7` (`probe-static-instance.md`, Finding 1). Secondary: static-via-instance accepted (P1); `static function` never emitted (P1) | `src/checker/mod.rs:46` add `is_static` to `FnSig` (set at `collect.rs:358`); `program.rs:~264` `cur_class.take()` for static bodies (mirror `:184`) → `E-STATIC-THIS`; `calls.rs` reject static-via-instance + add class-name static call path; emit `static function` in transpiler |
| 6 | **P0** | Value-returning statement-body **lambda** must return on all paths | Free fns + methods enforce `E-MISSING-RETURN`; `check_lambda`'s `LambdaBody::Block` arm never calls `check_return_totality`. A `: int` lambda that falls off the end binds `unit` into an `int` slot on both backends. | `phg check p6b-lambda.phg → OK` (same body as a free fn → `E-MISSING-RETURN`); `phg run/runvm p6c → r = unit` (`probe-return-all-paths.md`, Part B) | `src/checker/expr.rs` `check_lambda` `LambdaBody::Block`/`Some(rt)` arm (~902-908): add `self.check_return_totality(&declared, stmts, span);`. Companion: route block through `check_body` for `W-UNREACHABLE`. Front-end only |
| 7 | **P1** | Duplicate field / promoted field / parameter names must be rejected | Methods get `E-OVERLOAD-DUPLICATE`; fields (explicit, promoted, cross-collision) and params have **no uniqueness pass** — all accepted, last-declaration/last-arg silently wins. | `phg run dup-fields.phg → 2` (2nd promoted wins); `phg check dup-params.phg → OK`, `add(3,5) → 5` (`probe-dup-decl.md`) | `src/checker/collect.rs` — uniqueness pass for fields+promotions (~:1278/:1299) → `E-DUP-FIELD`; shared `reject_dup_param_names` at free-fn `:914`, method `:1337`, ctor `:1289` → `E-DUP-PARAM`. **Escalation watch:** duplicate name with *different types* (`int a, string a`) not probed — possible P0 if checker binds first type, runtime binds last arg |

---

## 3. ENFORCED (verified-sound) rules — the clean list

These rejected the bad program for the **right reason** with a dedicated diagnostic, and the
positive control passed. No action needed.

| Rule | Code | Probe |
|------|------|-------|
| Cannot instantiate an `abstract class` (incl. empty-abstract edge) | `E-ABSTRACT-INSTANTIATE` | `probe-abstract-new.md` |
| Final-by-default: cannot `extend` a non-`open` class | `E-EXTEND-FINAL` | `probe-final-extend.md` |
| Cannot reassign an immutable local / class `const` (direct, `+=`, `var`-inferred) | `E-ASSIGN-IMMUTABLE` / `E-CONST-REASSIGN` | `probe-const-reassign.md` |
| Cannot mutate a non-`mutable` field (direct, alias, `this.f=`) | `E-ASSIGN-IMMUTABLE` | `probe-immutable-field.md` |
| `implements` requires matching method signature (return/arity/param, incl. via `extends`) | `E-IFACE-SIG` | `probe-iface-sig.md` |
| `match` over enum/union/primitive-union is exhaustive (catch-all required for infinite domains) | "non-exhaustive match: missing …" | `probe-match-exhaust.md` |
| Enum variant construct + match arity (too-few/too-many/zero-with-arg) | "expects N argument(s)/field(s)" | `probe-enum-arity.md` |
| `private` method not callable cross-class / from `main` | `E-METHOD-VISIBILITY` | `probe-private-method.md` |
| `private` field not readable/writable cross-class / from `main` | `E-FIELD-VISIBILITY` | `probe-private-field-rw.md` |
| Value-returning **free fn / method** returns on all paths (incl. `while(true)+break` non-divergence) | `E-MISSING-RETURN` | `probe-return-all-paths.md` (Part A) |

Additionally confirmed enforced during discovery (not separately Stage-2 probed, but evidenced in
`discover-checker.md`): `E-IFACE-UNIMPL`, `E-ABSTRACT-UNIMPL`, `E-OVERRIDE-FINAL`,
`E-CONST-VISIBILITY`, `E-CONST-INSTANCE-ACCESS`, static-field-via-instance miss, `E-OPT-ASSIGN`
(null into non-optional), `E-STATIC-NO-INIT`. Parser-side enforced (`discover-parser.md`):
interface-member modifiers rejected at parse, property-hook stray modifiers rejected, top-level
visibility enforced at the importer boundary (`E-VIS-PRIVATE`/`E-VIS-INTERNAL`), `T??` is a parse
error (no silent collapse).

**Cosmetic note (P3, not soundness):** member-visibility diagnostics raised inside string
interpolation (`"{a.balance}"`) report the caret at the `package` line (`1:2`) instead of the
access site — text/code/hint correct. Fix by propagating the `StrSeg::Interp` absolute offset.

---

## 4. Finding #1 — the private-constructor bug (the 7th visibility access site)

Phorj already routes **six** external member-access surfaces through the checker's
`enforce_member_vis` chokepoint (memory `member-visibility-six-access-sites`): field read, field
write, clone-with, let-destructure, match-struct-pattern, method call — all correctly emit
`E-FIELD-VISIBILITY` / `E-METHOD-VISIBILITY`. **Construction is the missing seventh.**

The defect is **structural, not a missed check**: `src/parser/items.rs:510-527` parses the
modifiers, then the `TokenKind::Constructor` arm builds `ClassMember::Constructor { params, body,
span }` **without threading `modifiers`**, and the AST node has no visibility field. The
information is destroyed before any backend sees it (the comment literally says "consumed and
dropped"). So `private`/`protected`/`public`/`static`/`abstract`/`const`/`open`/`mutable` on a
constructor are *all* no-ops.

```
$ phg check private-ctor.phg → OK (type-checks clean)   exit=0
$ phg run   private-ctor.phg → 42                        exit=0
$ phg runvm private-ctor.phg → 42                        exit=0
```

PHP itself enforces `protected __construct` (the factory-method pattern), so this is a regression
against the very baseline Phorj claims to improve. **Fix = the 7th access site:** add
`visibility` to the constructor AST node (parser), then gate `new C(...)` in the checker through
the same `enforce_member_vis` scope logic, emitting `E-CTOR-VISIBILITY`. Front-end-only — no new
`Op`, no `Value`, byte-identity spine untouched (the construction site is identical; the checker
merely gains a rejection path). The transpiler should also emit the visibility keyword on the
generated PHP `__construct` (mind the multi-parent trait-lowering `__construct` path).

---

## 5. Candidate rules discovery surfaced but did NOT individually probe (follow-up)

Each below is evidenced in a discovery raw but lacks a dedicated Stage-2 probe with full controls.
Recommend probing before the fix batches land (most share a root cause with a confirmed gap).

| Candidate | Discovery evidence | Likely sev | Note |
|-----------|--------------------|-----------|------|
| `abstract constructor` accepted (with a body) — nonsense + unsound shape | `discover-parser.md` C1b: `phg check p3_abstract.phg → OK` | P1 | Same root cause as finding #1 (ctor modifiers dropped); the fix's "reject non-visibility modifiers on a ctor" closes it |
| `static`/`const`/`open`/`mutable`/`public constructor` silently ignored | `discover-parser.md` C1c: all `→ OK` | P1/P2 | Same root cause; closed by the same parser change |
| Contradictory / duplicate member modifiers (`public private function`) accepted, `public` silently overridden | `discover-parser.md` C2: `phg check p7_dup_mod.phg → OK`, resolves to private | P2 | `parse_modifiers` (`items.rs:619-641`) has no dedupe/conflict gate; not unsound (deterministic) but a declared-`public`-ignored footgun |
| Meaningless `const`/`mutable` on a method accepted, no effect | `discover-parser.md` C3: `phg check p8_const_method.phg → OK` | P2 | Property-hook path *does* reject stray modifiers — methods/fields lack the equivalent allow-list |
| Unassigned **optional** field (`int? n`) faults on read instead of yielding `null` | `discover-checker.md` GAP-3: `phg run p21.phg → no field n on C` | P1 | Facet of finding #4; sound fix = default unassigned optional fields to `null` (or require init) |
| Same reflexive-edge hole for **container heads** — `List<int> = List<string>`, Map key/value, `Optional`/`Function` type-arg variance | `discover-checker.md` handoff note | P0 (likely) | Same `src/types.rs:228` short-circuit; finding #2's fix probably closes them but each needs a confirming probe |
| Duplicate param/field with **different** types (`int a, string a`) | `probe-dup-decl.md` escalation note | P0 (if confirmed) | Could become a type hole if checker binds first type, runtime binds last arg — escalates finding #7 |
| Conditionally-assigned field (no definite-assignment flow analysis) | `discover-checker.md` GAP-2: `p4c.phg` ctor `if(flag){this.n=7}` called `false` → faults | P0 | Part of finding #4's scope; the flow-join fix must cover it |

---

## 6. Recommended FIX BATCHES

All confirmed gaps are **front-end-only** (checker/parser + one transpiler emission) — **none add
an `Op` or change a `Value`, so the byte-identity `run≡runvm≡PHP` spine is preserved.** Grouped by
shared mechanism:

**Batch A — Constructor visibility (closes finding #1 + the §5 abstract/static/… ctor variants).**
[Verified: root cause read in `src/parser/items.rs:510-527`; six-site precedent confirmed via
memory + probes] One AST field + one parser change + one checker gate + transpiler keyword.
Front-end + transpiler. Ship `E-CTOR-VISIBILITY` + `phg explain` + guide example.

**Batch B — Generic / nominal invariance (closes finding #2; likely the §5 container-head
variants).** [Verified: exact line `src/types.rs:228` and the reflexive `is_subtype` at
`collect.rs:846` read in `probe-generic-invariance.md`] Single same-head split. Pure front-end.
Add probes for List/Map/Optional/Function heads to confirm one fix covers them.

**Batch C — `throws` on method calls (closes finding #3).** [Verified: method overload tuple
`(Vec<Ty>,Ty)` drops `throws`, no `discharge_call_throw` in the method path] Widen the tuple,
discharge in `check_method_sigs`, wire method-call `?`. Pure front-end. Highest *idiomatic* blast
radius — every `class … function f() throws E`.

**Batch D — Definite assignment of instance fields (closes finding #4 + §5 optional-field-null +
conditional-assign).** [Verified: `check_type_body` has no field-init pass; static fields already
require init] Reuse the totality `stmt_terminates`/path-join engine for the "on all paths"
semantics; decide the optional-field policy (default-null vs require-init) in the same pass. Pure
front-end.

**Batch E — Static method semantics (closes finding #5).** [Verified: `FnSig` has no static flag;
`cur_class` left set for static bodies; transpiler drops `static`] `is_static` on `FnSig` +
`cur_class.take()` for static bodies + call-site separation + `static function` emission. Touches
the **transpiler** (the one emission change) but is byte-identity-safe *after* the `this`-leak fix
(a sound static method never touches `$this`).

**Batch F — Totality for lambdas (closes finding #6).** [Verified: `check_lambda` block arm omits
`check_return_totality`] One added call. Pure front-end.

**Batch G — Declaration uniqueness (closes finding #7 + §5 different-type escalation).** [Verified:
no uniqueness pass for fields/promotions/params; methods have `E-OVERLOAD-DUPLICATE`] Field +
promotion uniqueness + shared param-dedupe helper. Pure front-end. **Probe the different-type case
first** — if it's a type hole, re-grade this batch P0.

**Suggested order (impact × idiomatic reach):** A (the seed, complete the visibility matrix) →
C (`throws` — biggest real-code surface) → B (generic hole) → D (field init) → F (lambda totality,
one line) → E (static) → G (uniqueness). Each ships green + byte-identical with a guide example per
the examples-ship-with-features rule.

**Severity rollup [Verified — from pasted probe output]:** 6 P0 + 1 P1 confirmed gaps; 10 rules
enforced; 8 candidate rules await a confirming probe (3 of which could escalate to P0).
