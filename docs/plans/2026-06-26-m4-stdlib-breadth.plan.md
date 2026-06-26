# M4 — stdlib breadth (plan)

> Active milestone chunk after the autonomous backlog closed (2026-06-26). Each native ships
> byte-identity-gated (run≡runvm≡real PHP 8.5) with a guide example, per the standing rules.

## Decisions Log
- [2026-06-26] Next chunk = **M4 stdlib breadth** (developer choice over M8 hardening / M-NUM / Json round 2).
- [2026-06-26] **Sort API = `sort` + `sortWith`** (developer, Option 1 — mirrors PHP `sort`/`usort`):
  - `Core.List.sort(List<T>) -> List<T>` — natural ascending; **byte-identity trap:** PHP `<=>` juggles
    numeric strings (`"10" <=> "9"` → numeric), so strings must compare via `strcmp` (byte/lexicographic,
    matching Rust `String` Ord); int/float via numeric `<=>` (matching Rust). Gated helper
    `__phorge_sort` (usort + type-dispatched comparator). NaN floats = documented edge → use `sortWith`.
  - `Core.List.sortWith(List<T>, (T, T) -> int) -> List<T>` — comparator (higher-order, reuses the
    map/reduce re-entrant closure machinery); erases to `usort($ys, $cmp)`. Stable (Rust + PHP 8.0+).
  - Both return a NEW list (Phorge lists are immutable/COW), so the PHP helper copies before `usort`.
- [2026-06-26] **Casting system** — sequencing: **sort now, casting spec next** (M4 Slice 2, spec-first,
  developer choice). NOT a C-style `(int)x` cast (the PHP surprise Phorge removes). Surface: developer
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
Phorge philosophy ([[philosophy-of-phorge]]) = legible, surprise-free PHP upgrade. PHP casts
(`(int)`, `(bool)`, type juggling) are a top surprise source → the Phorge answer is **explicit, named,
total-or-optional conversions**, not a C-cast operator:
- **Total/widening** (never fails): `int → float`, `int/float/bool → string`.
- **Partial/narrowing** (→ `T?`, surfaces failure, no silent truncation): `string → int?` (= existing
  `Text.parseInt`), `string → float?`, `float → int?` (lossy: explicit `truncate`/`round`, or `int?`).
- **Surface options:** a `Core.Convert` module (most consistent with namespaced stdlib + byte-identity
  control) vs a `x as T` operator (ergonomic but invites the C-cast surprise model — leaning against).
- Open: does Phorge already auto-widen `int → float` in arithmetic (`1 + 2.0`)? The spec must pin the
  implicit-coercion rules. Spec-first.

## Status
- [x] **Slice 1 sort/sortWith** — DONE (`examples/guide/sort.phg`, byte-identical; gated
  `__phorge_sort`/`__phorge_sort_with`; no new Op/Value).
- [ ] Slice 2 Casting — **NEXT: research + brainstorm spec** (mix of surfaces + TS-style `<X>`;
  separate the assertion vs conversion axes; pin implicit-coercion policy).
