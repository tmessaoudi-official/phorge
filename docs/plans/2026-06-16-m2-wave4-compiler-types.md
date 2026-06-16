# M2 Wave 4 — Class-aware compiler types (close the `num_ty` parity gap)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans (inline; subagents
> deadlock on the ask-human gate in this repo). Steps use checkbox (`- [ ]`) syntax. Phorge git
> autonomy applies (commit green, self-contained; see `CLAUDE.md`). Read `docs/INVARIANTS.md`
> before touching the compiler/backends. This is the long-deferred "Wave 4" from the M2 P3.5
> hardening roadmap, scheduled (per that roadmap) to land *with* P4/P5 — now, right after P4.

**Goal:** Close the last known `phorge run` ↔ `phorge runvm` divergences: programs that type-check
and run on the interpreter but the **VM compiler rejects** because its operand-type inference
(`num_ty`) is *coarse* (`TyTag = Int | Float | Other`) and can't see through a field read on an
arbitrary instance or a method-call result. Give the compiler a **class-aware** internal type so
`num_ty` resolves those, making `runvm` a faithful drop-in across the full checker-valid surface.

**Architecture:** The interpreter is the reference oracle; the differential harness is the spine.
Every binding in M1 carries a *declared* type annotation (params, `var`-decls, fields, fn/method
returns, for-loop vars), so the compiler can build a richer type **structurally from the AST
`Type`** — it does **not** need the checker's inference results. The change is compiler-internal
(`src/compiler.rs`); the `Op` set, the VM, and `value.rs` are untouched.

---

## Evidence — the real divergences (measured 2026-06-16)

| Program (arithmetic operand) | `run` (oracle) | `runvm` (today) | Verdict |
|---|---|---|---|
| `p.x + 1` (field of an arbitrary instance) | `8` | **compile error** `cannot infer numeric type of Member{…}` | **Gap — fix** |
| `c.get() + 1` (method-call result) | `6` | **compile error** `cannot infer numeric type of Call{Member…}` | **Gap — fix** |
| `xs[0] + 1` (list element) | runtime error `indexing is not yet supported in M1` | compile error | **Not a gap** — M1 has no user `a[i]` on *either* backend (out of surface) |
| `for (int x in xs) { x + 1 }` | `11 / 21` | `11 / 21` | Already correct (loop var is a typed local) |

So the scope is exactly **(A) field reads on arbitrary instances** and **(B) method-call results**
used where `num_ty` runs (the LHS of `+ - * / %`, recursively). List indexing is explicitly **out**
(not in the M1 interpreter surface).

## Scope (frozen)

**In:** a class-aware compiler type; `num_ty` resolution through `Member` (any instance, not just
`this`), method-call results, `this`, locals, enum-payload bindings, and free-function results that
carry a class type; the `(class, method) → return type` and `class → field → type` tables needed
for that resolution.

**Out:** list-element indexing (`a[i]` — not in M1 on either backend); `null`/`|>` (same); the
arena object model and GC (M2 P5); any `Op`/VM/`value.rs` change; richer-than-needed type tracking
(e.g. generic instantiation) — `Other` stays the catch-all for everything non-numeric/non-class.

## Design decisions (review these)

| # | Decision | Choice | Rationale |
|---|---|---|---|
| W4-1 | **Compiler type representation** | Replace coarse `enum TyTag { Int, Float, Other }` with `enum CTy { Int, Float, Class(String), Other }`. **No `List` variant** (indexing is out of M1, so a `List` local's element type is never an operand). | The only extra fact `num_ty` needs is *which class* an instance/field/result is, so it can look up the field/method's declared numeric-ness. Minimal superset of `TyTag`; `Int`/`Float`/`Other` semantics unchanged. |
| W4-2 | **Derive types structurally, not from the checker** | Build `CTy` from the AST's declared `Type` annotations (a richer `type_tag`/`resolve_cty`). | Every binding has an explicit annotation; the compiler already re-derives coarsely. No need to thread the checker's `types::Ty` or annotate the AST — keeps the AST untouched (it's shared with the interpreter/transpiler and is `PartialEq`). |
| W4-3 | **One resolver** | Add `ctype(&Expr) -> Result<CTy, String>` that resolves an expression's type (Ident→binding/local/`this`-field, `This`→current class, `Member`→field's `CTy` of the object's class, `Call{Ident}`→fn return, `Call{Member}`→method return). `num_ty` becomes the numeric projection of `ctype` (`as_num`). | Generalizes today's `num_ty` so each surface (field, method result, nested `a.b.c`) is handled once, recursively, instead of per-arm special cases. |
| W4-4 | **Carry `CTy` at every binding site** | `Local.ty`, `FnMeta.ret`, `VariantMeta.field_tags`, `MatchBinding.ty`, and the class field-type map all become `CTy`; add per-class `field → CTy` and `(class, method) → CTy` tables + a `cur_class: Option<String>` on the compiler (for `ctype(This)`). | A class-typed local/field/payload/return must remember its class so `class_of` can walk `obj.field`, `c.method()`, `match … Some(p) => p.x`, etc. Mechanical, uniform. |
| W4-5 | **List indexing stays rejected** | Leave `Index` unhandled in `num_ty`/`ctype` (errors). Document that `xs[i]` arithmetic faults on both backends (interpreter at runtime, VM at compile) — an *out-of-surface* construct, not a Wave-4 target. | M1 has no user indexing (interpreter rejects it); adding it is an M3 language-enrichment task, not a parity fix. Note the pre-existing cross-stage `agree_err` asymmetry is untested (no indexing program in the corpus). |

---

## File Structure

- **Modify** `src/compiler.rs` — the whole change: `TyTag` → `CTy` (add `Class`); `type_tag` →
  `resolve_cty`; add `ctype(&Expr)` + use it from `num_ty`; build `class_field_ctys` +
  `method_rets` in the pre-pass; add `cur_class`; thread `CTy` through `Local`/`FnMeta`/
  `VariantMeta`/`MatchBinding`/`field_tags`/`Compiler::new`/`compile_method`/`compile_constructor`.
- **Modify** `tests/differential.rs` — a `WAVE4_PROGRAMS` corpus: `p.x + 1`, `c.get() + 1`,
  nested `a.inner.x + 1`, an enum payload of class type used arithmetically
  (`match … Some(p) => p.x + 1`), a free function returning an instance then `f().x + 1`. All
  `agree` (Ok-path) — these are the programs that diverge today.
- **Modify** `CHANGELOG.md`, `CLAUDE.md` — mark Wave 4 done; update the baseline test count and the
  "`num_ty` coarse-gap" notes (the gap is now closed for the in-surface cases).
- **No change** to `src/chunk.rs`, `src/vm.rs`, `src/value.rs`, the `Op` set, or the AST.

> No new `Op` ⇒ the three-exhaustive-match coupling does **not** apply to this wave.

---

## Phasing — one TDD-first, parity-gated commit

This is a cohesive compiler-internal refactor (the `TyTag → CTy` swap is atomic — it can't be
half-applied), so it lands as **one** green commit.

- [x] **W1 (test):** added `WAVE4_PROGRAMS` (5 cases) + `wave4_programs_match_between_backends`.
      Confirmed **red** (interpreter `Ok`, VM `compile error: cannot infer numeric type`). As-built:
      the planned no-payload `None` variant was vacuous (bare zero-arg variants fail the checker on
      *both* backends — "unknown identifier `None`"), so case (D) uses two payload-bearing variants
      (`Some(Point p), Zero(int z)`), keeping the class-typed-payload coverage non-vacuous.
- [x] **W2:** introduced `enum CTy { Int, Float, Class(String), Other }`; `type_tag` →
      `resolve_cty` (Named `int`/`float` → `Int`/`Float`; known primitives/containers
      `bool`/`string`/`void`/`List`/`Map`/`Set` → `Other`; any other `Named` → `Class`;
      `Optional` → `Other`). Threaded `CTy` through `Local`/`FnMeta.ret`/`VariantMeta.field_tags`/
      `MatchBinding.ty`/the class field-type map/`Compiler::new`.
- [x] **W3:** pre-pass builds `class_field_ctys: HashMap<String, HashMap<String, CTy>>` (keyed by
      class *name* for arbitrary-instance resolution) and `method_rets: HashMap<(String, String),
      CTy>`; `cur_class` set in `compile_method`/`compile_constructor`.
- [x] **W4:** added `ctype(&Expr) -> Result<CTy, String>`; reimplemented `num_ty` as
      `as_num(self.ctype(e)?)` (fault wording preserved). As-built: no separate `class_of` helper —
      the `Member`/method-call arms match on `self.ctype(object)?` inline (simpler, same effect).
      The P4c `this.field`-only `Member` arm is removed (subsumed by the general `ctype(Member)`).
      `compile_match`'s scrutinee-type now uses `ctype` so a class-typed catch-all binding resolves.
- [x] **W5:** suite green (**244** = 243 + the new corpus), `cargo clippy --all-targets` clean,
      `cargo fmt --check` clean. Commit: `refactor(compiler): class-aware types — close num_ty parity gap (M2 Wave 4)`.

## Acceptance criteria

- The two divergent probes (`p.x + 1`, `c.get() + 1`) and the `WAVE4_PROGRAMS` corpus are
  byte-identical on `run` and `runvm`.
- All prior tests stay green; no `Op`/VM/`value.rs`/AST change; clippy + fmt clean;
  `#![forbid(unsafe_code)]` intact.
- `CLAUDE.md`/`CHANGELOG.md` no longer claim the `this.field`-only `num_ty` limitation; the only
  remaining documented coarse-type note is the deliberately out-of-surface `Index`.

## Risks & rollback

- **Risk — broad mechanical churn:** `TyTag` appears at ~6 sites. *Mitigation:* it's a superset
  swap (existing variants keep their meaning); the 243-test suite + the differential harness catch
  any regression; one atomic commit, `git revert`-able.
- **Risk — `ctype` infinite recursion / unhandled expr:** *Mitigation:* `ctype` mirrors `expr`'s
  finite recursion; any unresolved expr returns the same "cannot infer numeric type" error as today
  (no behavioral regression — only *more* cases now succeed).
- **Risk — a class-typed value used non-numerically:** `as_num(Class)` → `None` → the existing
  "`x` is not numeric" fault. Unchanged behavior for non-numeric operands.
- **Rollback:** single isolated commit; revert restores the P4c state (the documented gap returns
  but nothing else changes).

---

## Decisions Log

- [2026-06-16] AGREED: Do **Wave 4 before M2 P5** — a correctness/parity gap outranks a (bench-
  gated, already-met) perf milestone, it is lower-risk and self-contained, and it settles the
  compiler's type model before P5 churns the object representation.
- [2026-06-16] AGREED: Wave 4 scope is **field-reads-on-arbitrary-instances + method-call results**
  used as arithmetic operands (the two measured divergences). **List indexing is out** — not in the
  M1 surface on either backend.
- [2026-06-16] AGREED: derive the class-aware type **structurally from AST `Type` annotations**
  (add `CTy::Class`), not by threading the checker's `types::Ty` or annotating the shared AST.
