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

## Pinned completion backlog (autonomous — full-auto bypass active, 2026-06-26)

> Developer pinned **"finish M4 = `as` operator + stdlib breadth sweep"** as the next big chunk. Run
> hands-off, commit green slices, NEVER `git push`, pause only on genuine design forks (→ AskUserQuestion).
> Each item byte-identity-gated (run≡runvm≡real PHP 8.5) + a guide example, per standing rules.

1. **`as` operator (Slice 2b)** — speced in `docs/specs/2026-06-26-m4-casting-conversion-design.md`.
   `v as T` ⇒ `T?` (checked, Kotlin/Swift model). Reuse `Op::IsInstance` (NO new Op): lower to
   `if (v instanceof T) Some(v) else None`; transpile `($v instanceof T ? $v : null)`. Scrutinee =
   class/interface/union; primitive `as` rejected (`E-CAST-TYPE`). `as` is contextual (import-alias
   reuse). Precedence tighter than `??`, looser than member. Smart-cast in `if (var t = v as T)`.
   `phg explain E-CAST-TYPE`; `examples/guide/as-cast.phg`.
2. **Map mutation/access** — `Core.Map` is read-only today (keys/values/has/size; `m[k]` reads &
   faults on miss). Add `set(Map<K,V>, K, V) -> Map<K,V>` (new map, COW), `remove(Map<K,V>, K) ->
   Map<K,V>`, `get(Map<K,V>, K) -> V?` (safe, None on miss; V is non-optional so a present value is
   never null). PHP: assoc set / `unset` over a copy / `array_key_exists ? : null`.
3. **List breadth** — `slice(List<T>, int, int) -> List<T>` (array_slice, clamp), `indexOf(List<T>, T)
   -> int?` (array_search strict → None on miss), `concat(List<T>, List<T>) -> List<T>` (array_merge),
   `first`/`last(List<T>) -> T?`.
4. **Text breadth** — `padLeft`/`padRight(string, int, string) -> string` (str_pad), `indexOf(string,
   string) -> int?` (strpos → None), `substring(string, int, int) -> string` (substr, byte-safe /
   tier-1, no mbstring — see [[transpile-no-ini-extensions]]).
5. **Set ops** — `union`/`intersection`/`difference(Set<T>, Set<T>) -> Set<T>` (insertion-ordered Set
   discipline; PHP array_unique/array_intersect/array_diff). Deferred since S7b.
6. **`Text.parseFloat(string) -> float?`** — gated helper matching Rust `f64::from_str`. **Possible
   pause:** inf/nan/`.5`/`5.` acceptance is a genuine fork (match Rust permissive, or stricter
   JSON-like?) — surface via AskUserQuestion if non-obvious.

**Decisions Log (this chunk):**
- [2026-06-26] Big chunk = **finish M4** (`as` + stdlib sweep), full-auto, over M8 hardening / M-NUM/M-TIME.

## Status
- [x] **Slice 1 sort/sortWith** — DONE (`examples/guide/sort.phg`, byte-identical; gated
  `__phorge_sort`/`__phorge_sort_with`; no new Op/Value).
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
- [ ] **Slice 2b** — the checked `as` operator (language change; reuse `Op::IsInstance`).
