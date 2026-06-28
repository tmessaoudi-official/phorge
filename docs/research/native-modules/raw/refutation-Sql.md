# Stage 2b — Adversarial byte-identity review of `Core.Sql` (typed query builder)

**Verdict on the byte-identity claim: it HOLDS — but the spike's headline API and its
example DO NOT COMPILE today.** `determinism_holds = true` (the parameterize-don't-inline
strategy genuinely removes the only escaping divergence surface). `revised_feasibility` is
LOWER than 92% — not because of any determinism risk, but because two load-bearing
prerequisites the spike treats as minor are, on the actual codebase, hard compile errors:
(1) the heterogeneous bind list `[18, true]` is rejected by the checker, and (2) the example's
print path (`Debug.dump`) does not exist. Tier stays **A**.

---

## What I verified in the codebase (Verified, with file:line)

1. **String + List ARE byte-identical primitives — the spine claim is true.**
   - `Value::as_display` (`src/value.rs:241-255`): `Int(n) => n.to_string()` (matches PHP `(string)int`),
     `Bool(b) => b.to_string()` → `"true"/"false"`, `Str(s) => s.clone()`. A printed **bool** is
     reconciled across legs by the `__phorj_str` runtime helper
     (`src/transpile/program.rs:295-301`: `if (is_bool($v)) return $v ? "true" : "false";`), so a
     bound `true`/`false` rendered into output is identical run/runvm/PHP. **No float on the build
     path** (binds carried, not formatted) — the spike's risk #5 is correctly N/A.
   - `Text.join → implode(glue, array)` (`src/native/text.rs:335-341`), `Text.replace →
     str_replace(search, replace, subject)` (`src/native/text.rs:345-350`) — both PHP **core**,
     survive `php -n`. Identifier quoting `'"' . str_replace('"','""',$id) . '"'` is core-only.
     **§5 `php -n` safety holds.** No PDO, no mbstring, no ext on the build path. Confirmed.
   - Placeholder `?` and ANSI `"ident"` are literal strings the Phorj code writes → identical by
     construction. **No escaping divergence surface (risk #1) — genuinely eliminated.** Agreed.

2. **REFUTATION 1 (P0 — blocks the public API as sketched): `[18, true]` does not type-check.**
   `src/checker/expr.rs:674-691` `check_list`: the checker infers the first element's type and
   **errors `"list elements must share one type; found `int` and `bool`"`** on any mixed list.
   The spike's §6 example (`.where("age > ?", [18]).where("active = ?", [true])`) passes
   homogeneous singletons so it sidesteps this — but the moment a real query binds mixed types in
   one call (`.where("a = ? AND b = ?", [18, "x"])`, the normal case) it is a **compile error**.
   The spike calls this "the one genuine language gap … 8%" and frames it as ergonomics; it is in
   fact the load-bearing constraint on the whole public surface. A `List`-of-mixed-binds cannot
   exist. This is not a byte-identity defect — it is a feasibility-of-the-stated-API defect.

3. **REFUTATION 2 (P0 for the example, not for the spine): `Debug.dump` does not exist.**
   The §6 example prints binds via `Console.println(Debug.dump(q.params()))`. There is **no
   `Debug`/`Dump` native** (`src/native/` has no debug.rs; `grep -ri dump src/native` = none) — it
   is a *separate, still-unbuilt* feasibility spike (`docs/research/native-modules/raw/feasibility-Dump.md`).
   Worse, a bare `List` **cannot be printed at all**: `as_display` returns `None` for
   `Value::List` (`src/value.rs:254` `_ => None`) and the checker rejects interpolating it —
   `"type `list` cannot be interpolated into a string"` (`src/checker/expr.rs:540`). So the example
   that the differential harness would glob (`examples/sql/*.phg`) **cannot be written as sketched**;
   the author must hand-roll a per-element stringification (iterate + `match` a `Bind` enum) to
   produce any output to gate. That output is still byte-identical once written — but the spike's
   "the example prints `q.params()`" is not achievable verbatim.

4. **REFUTATION 3 (P1 — the mitigation is heavier than claimed): no precedent for injecting a
   multi-method builder class.** Every injected prelude that ships is a single **enum** with no
   method bodies — `JSON_PRELUDE` (`src/cli/mod.rs:294`) and `ROUNDING_MODE_PRELUDE`
   (`src/cli/mod.rs:338`). The spike proposes injecting an entire `Query`/`Sql` **class pair with
   chained `from`/`where`/`orderBy`/`limit`/`build` method bodies written in Phorj**. That is a
   materially larger and unproven prelude (clone-with semantics, private-field reads, cross-package
   `List<string>` fields, a `match` over a `Bind` enum inside `build()` to render placeholders). It
   may well work — but "copy of `inject_json_prelude`" (§9) understates it by an order of magnitude.
   The spike's own §6 mitigation ("inject a `Bind` sum type") is the *correct* path and is the same
   injected-enum trick — but then the builder's `where(string, List<Bind>)` signature forces every
   call site to wrap binds (`[Bind.I(18), Bind.S("x")]`), which is the real ergonomics cost and is
   exactly what the unbuilt `Any` type was meant to avoid.

## Determinism hunt — nothing new found beyond the spike's own list

I actively hunted the named hazards; none breaks the spine **on the build path**:
- **Object ids / addresses:** none — output is `string` + a `List` of scalar binds; no `Instance`
  identity is printed. (A future `Debug.dump` of an *instance* is the Dump spike's problem, not Sql's.)
- **Hash-map ordering:** N/A — the builder uses insertion-ordered `List`, not `Map`, for binds and
  clauses. `Value::List` is `Rc<Vec>` (ordered). Correct.
- **Float formatting (Rust Ryū vs PHP precision=14):** N/A on build path (binds carried, never
  formatted) — *provided* LIMIT/OFFSET stay `int` (spike §8 #7 pins this; correct). A float bind
  only meets the divergence if a downstream Tier-B `Core.Db` stringifies it — out of scope. Holds.
- **Locale / clock / RNG:** none present. Holds.
- **`php -n` missing ext:** the build path is `implode`/`str_replace`/`.`/array ops — all core.
  Holds. (The trap would be the rejected `PDO::quote` inline design; correctly rejected.)
- **One byte of PHP drift:** the transpiled methods are ordinary string concat + array append; a
  printed `int`/`bool`/`string` is reconciled by `__phorj_str`. No SQL-specific PHP builtin emitted.
  Holds.

## Net assessment

The **byte-identity claim is correct** and the strategy ("free by construction, no second escaping
impl") is sound — I could not refute determinism. What I refute is the **92% / "small" framing**:
the spike buries two compile-blocking prerequisites (heterogeneous binds rejected at
`checker/expr.rs:683`; no `Debug.dump` and lists are non-printable at `checker/expr.rs:540`) under an
"ergonomics decision" label. Until a `Bind` injected enum (or the deferred `Any`) ships AND a
list-printing surface exists, the module's *public API and its gating example cannot be written as
sketched*. Feasibility of a byte-identical `Core.Sql` is real, but gated behind `Bind` (or `Any`) +
either `Core.Debug` or a hand-rolled per-element printer. Revised feasibility ~78%, still Tier A.
The 14-point drop is all "the stated API doesn't compile yet," not determinism.
