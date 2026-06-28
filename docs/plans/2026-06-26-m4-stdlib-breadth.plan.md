# M4 — stdlib breadth (plan)

> Active milestone chunk after the autonomous backlog closed (2026-06-26). Each native ships
> byte-identity-gated (run≡runvm≡real PHP 8.5) with a guide example, per the standing rules.

## Decisions Log
- [2026-06-26] Next chunk = **M4 stdlib breadth** (developer choice over M8 hardening / M-NUM / Json round 2).
- [2026-06-26] **Sort API = `sort` + `sortWith`** (developer, Option 1 — mirrors PHP `sort`/`usort`):
  - `Core.List.sort(List<T>) -> List<T>` — natural ascending; **byte-identity trap:** PHP `<=>` juggles
    numeric strings (`"10" <=> "9"` → numeric), so strings must compare via `strcmp` (byte/lexicographic,
    matching Rust `String` Ord); int/float via numeric `<=>` (matching Rust). Gated helper
    `__phorj_sort` (usort + type-dispatched comparator). NaN floats = documented edge → use `sortWith`.
  - `Core.List.sortWith(List<T>, (T, T) -> int) -> List<T>` — comparator (higher-order, reuses the
    map/reduce re-entrant closure machinery); erases to `usort($ys, $cmp)`. Stable (Rust + PHP 8.0+).
  - Both return a NEW list (Phorj lists are immutable/COW), so the PHP helper copies before `usort`.
- [2026-06-26] **Casting system** — sequencing: **sort now, casting spec next** (M4 Slice 2, spec-first,
  developer choice). NOT a C-style `(int)x` cast (the PHP surprise Phorj removes). Surface: developer
  wants a **mix** (Core.Convert module + `as` operator + UFCS methods) **plus a TS-style `<X>` form**;
  explicitly wants to **research + brainstorm** it. **Key distinction to explore in the spec:** TS
  `<X>v` / `v as X` are compile-time *type assertions* (no runtime conversion) — a different axis from
  *value conversion* (`int→float`, `string→int?`). A solid design likely separates the two axes cleanly
  and decides which surface serves which. Also pin: implicit coercion (today `1 + 2.0` is a hard type
  error — no auto-widening) — does the casting system relax that, or stay explicit? Spec-first, with
  research.

## Slice 1 — Core.List.sort + sortWith (locked, ready to build)
TDD: kernel tests (sort int/string/float ascending, stability, sortWith comparator + fault parity),
guide example `examples/guide/sort.phg` (byte-identity-gated 3-way). Add a gated `uses_list_sort` helper.

## Casting system — design notes (under discussion)
Phorj philosophy ([[philosophy-of-phorj]]) = legible, surprise-free PHP upgrade. PHP casts
(`(int)`, `(bool)`, type juggling) are a top surprise source → the Phorj answer is **explicit, named,
total-or-optional conversions**, not a C-cast operator:
- **Total/widening** (never fails): `int → float`, `int/float/bool → string`.
- **Partial/narrowing** (→ `T?`, surfaces failure, no silent truncation): `string → int?` (= existing
  `Text.parseInt`), `string → float?`, `float → int?` (lossy: explicit `truncate`/`round`, or `int?`).
- **Surface options:** a `Core.Convert` module (most consistent with namespaced stdlib + byte-identity
  control) vs a `x as T` operator (ergonomic but invites the C-cast surprise model — leaning against).
- Open: does Phorj already auto-widen `int → float` in arithmetic (`1 + 2.0`)? The spec must pin the
  implicit-coercion rules. Spec-first.

## Pinned completion backlog (autonomous — full-auto bypass active, 2026-06-26)

> Developer pinned **"finish M4 = `as` operator + stdlib breadth sweep"** as the next big chunk. Run
> hands-off, commit green slices, NEVER `git push`, pause only on genuine design forks (→ AskUserQuestion).
> Each item byte-identity-gated (run≡runvm≡real PHP 8.5) + a guide example, per standing rules.

1. **`as` operator (Slice 2b)** — ✅ **DONE** (see Slice 2b in Status above). `v as T` ⇒ `T?`, no new
   `Op`, byte-identical 3-way, single-eval proven, foreach-`as` ambiguity fixed. `examples/guide/as-cast.phg`.
2. **Map mutation/access** — ✅ **DONE** (`examples/guide/map-ops.phg`, byte-identical 3-way).
   `get(Map<K,V>, K) -> V?` (inline `($m[$k] ?? null)`), `set`/`remove -> Map<K,V>` (new map, COW;
   gated `__phorj_map_set`/`_remove`; `set` reuses `value::map_set`). No new `Op`/`Value`.
3. **List breadth** — ✅ **DONE** (`examples/guide/list-ops.phg`, byte-identical): `slice` (array_slice,
   normalized), `indexOf -> int?` (gated `__phorj_index_of`), `concat` (array_merge), `first`/`last -> T?`.
4. **Text breadth** — ✅ **DONE** (`examples/guide/text-ops.phg`, byte-identical): `padLeft`/`padRight`
   (str_pad, byte), `indexOf -> int?` (gated `__phorj_text_index_of`), `substring` (substr, byte). Tier-1.
5. **Set ops** — ✅ **DONE** (`examples/guide/set-ops.phg`, byte-identical): `union`/`intersection`/
   `difference` (array_unique∘array_merge / array_intersect / array_diff; first-set order).
6. **`Text.parseFloat(string, bool permissive = false) -> float?`** — ✅ **DONE** (shipped with the
   default-parameters feature, `docs/plans/2026-06-26-default-parameters.plan.md`; example
   `examples/guide/default-params.phg`, byte-identical 3-way). **M4 pinned backlog is now COMPLETE.**
   Original blocker note retained below for history:
   ⛔ ~~**BLOCKED on a new language feature.**~~ Developer
   chose `parseFloat(string, bool permissive = false)` (strict default, opt-in lax) over strict-only /
   two-named. That **requires default parameter values**, which Phorj lacks (`Param` has no default
   field; arity is checked exactly). So **default parameters is now a prerequisite language feature**
   (plan `docs/plans/2026-06-26-default-parameters.plan.md`, design
   `docs/specs/2026-06-26-default-parameters-design.md`). Strict grammar locked: rejects `inf`/`nan`
   (byte-identity — PHP can't match Rust's inf/nan parse), accepts `[+-]?digits(.digits)?([eE][+-]?digits)?`;
   permissive additionally accepts a leading/trailing dot (`.5`/`5.`).

**Decisions Log (this chunk):**
- [2026-06-26] Big chunk = **finish M4** (`as` + stdlib sweep), full-auto, over M8 hardening / M-NUM/M-TIME.
- [2026-06-26] AGREED (after two challenge rounds): **parseFloat = `parseFloat(string, bool permissive =
  false)`** — strict by default, opt-in permissive. Both reject inf/nan. Chosen over strict-only (my
  rec) and two-named fns. **Consequence:** needs **default parameters** as a language feature first →
  new prerequisite slice (see item 6). parseFloat ships as that feature's showcase.

## Status
- [x] **Slice 1 sort/sortWith** — DONE (`examples/guide/sort.phg`, byte-identical; gated
  `__phorj_sort`/`__phorj_sort_with`; no new Op/Value).
- [x] **Slice 2 design** — DONE: spec `docs/specs/2026-06-26-m4-casting-conversion-design.md`.
  Locked (developer): **checked `as` → `T?`** (decline TS unchecked); **no implicit coercion**;
  conversion via **`Core.Convert`** (UFCS makes it module+method in one); `to*` from typed values,
  `parse*` (fallible, from string) stays in `Core.Text`.
- [2026-06-26] **Module name = `Core.Convert`** (developer confirmed over `Core.Cast` after challenge):
  the `as` operator is the real "cast" (reinterpret); the module does value *conversion* (= .NET
  `System.Convert` / Rust `From` / Kotlin `toInt`), and "cast" stays one concept = the operator.
- [x] **Slice 2a** — `Core.Convert` natives DONE (`toString`/`toFloat`/`truncate`/`round`,
  `examples/guide/convert.phg`, byte-identical incl. UFCS `n.toFloat()`). `Text.parseFloat` deferred
  (fiddly inf/nan/`.5` byte-identity — a follow-up like parseInt).
- [x] **Slice 2b** — the checked `as` operator. **DONE** (`examples/guide/as-cast.phg`, byte-identical
  3-way + single-eval proven by a side-effecting scrutinee; `phg explain E-CAST-TYPE`; no new `Op`).
  **Regression found + fixed:** `as` is contextual (foreach `as`-separator vs cast) — added a parser
  `no_as_cast` restriction (set in `parse_foreach`, reset by every `parse_expr` so brackets re-enable
  casts; Rust no-struct-literal pattern). 930 lib + 109 differential (PHP-8.5 oracle) green.
  Implementation map (8 touch points, no new `Op`/`Value`):
  1. `src/ast/mod.rs` — new `Expr::Cast { value, type_name, span }` (mirrors `InstanceOf`).
  2. `src/ast/walk.rs` + `checker/expr.rs::expr_span` — Cast arms (free-vars + span).
  3. `src/parser/exprs.rs` — fold `Ident("as")` in `parse_binary` at prec 8 (== instanceof level),
     single type-name RHS → `Expr::Cast`. `support.rs` sexpr `(as v T)` + parser test.
  4. `src/checker/expr.rs::check_cast` — left operand class/union/intersection (else E-CAST-TYPE),
     RHS class/interface (primitive `as` rejected → guide to Convert), result `Ty::Optional(Named(T,
     erased-args))`. if-let smart-cast is inherited (T? narrows to T) — no narrow_from_condition arm.
  5. `src/interpreter/expr.rs` — eval value once, instanceof predicate, value-or-`Value::Null`.
  6. `src/compiler/expr.rs` — `??`-style scratch-slot (`self.height-1`) + `Op::IsInstance` + branch
     (value once); no `ctype` arm (T? is not an arithmetic operand, like instanceof→bool).
  7. `src/transpile/expr.rs` — arrow-IIFE `(fn($__as) => $__as instanceof T ? $__as : null)(<v>)`
     (evaluates `<v>` once — PHP byte-identity for side-effecting scrutinees; uses `type_pos_ref`).
  8. `src/cli/explain.rs` E-CAST-TYPE + `examples/guide/as-cast.phg` + README + KNOWN_ISSUES/CHANGELOG.
