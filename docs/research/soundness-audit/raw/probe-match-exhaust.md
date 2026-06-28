# Probe — match over enum/union is exhaustive

**Rule:** A `match` over an enum or a union type, with **no catch-all (`_`) arm**, must be
**rejected** at check time if it omits any variant/member. This is a core soundness property: an
un-handled case falling through silently would be a `run`↔`runvm`-divergent / type-hole defect.

**Verdict: ENFORCED (not a gap).** Severity: **none**.

Enforcement lives in `src/checker/matches.rs` (the messages `non-exhaustive match: missing …` are
produced there; a runtime `FaultMsg::NonExhaustiveMatch` in `src/chunk.rs` / `src/interpreter/expr.rs`
is the belt-and-braces backstop, never reached because the checker rejects first).

BIN=/stack/projects/phorj/target/release/phg

---

## Probe 1 — non-exhaustive ENUM match (missing `Defend`, no `_`)

Program `$TMP/match-exhaust.phg`: `resolve(Action)` matches `Attack`/`Heal` only, omitting `Defend`.

```
=== CHECK ===
type error at 11:12: non-exhaustive match: missing Defend
    return match a {
           ^
exit=1
=== RUN ===
type error at 11:12: non-exhaustive match: missing Defend
    return match a {
           ^
exit=1
```

Rejected for the RIGHT reason on both `check` and `run` (run runs the checker first). ENFORCED.

---

## Probe 2 — non-exhaustive UNION match (missing `Triangle`, no `_`)

Program `$TMP/match-exhaust-union.phg`: `area(Circle | Square | Triangle)` handles only `Circle`/`Square`.

```
=== CHECK ===
type error at 15:12: non-exhaustive match: missing Triangle
    return match s {
           ^
exit=1
=== RUN ===
type error at 15:12: non-exhaustive match: missing Triangle
    return match s {
           ^
exit=1
```

Union type-pattern exhaustiveness is enforced over the member set, exactly like an enum. ENFORCED.

---

## Probe 3 — PRIMITIVE-union match without catch-all (infinite domain)

Program `$TMP/match-prim.phg`: `classify(int | string code)` matches literals `0` and `"ok"` only,
with no `_`. The `int`/`string` domains are infinite, so literal arms can never exhaust them — a `_`
must be required.

```
=== CHECK ===
type error at 5:12: non-exhaustive match: missing int, string
    return match code {
           ^
exit=1
=== RUN ===
type error at 5:12: non-exhaustive match: missing int, string
    return match code {
           ^
exit=1
```

The checker correctly demands a catch-all for primitive-union scrutinees (it does NOT mistake a finite
set of literals for full coverage of an infinite type). ENFORCED — this is the subtle case and it holds.

---

## Probe 4 — positive control: COMPLETE enum match passes

Program `$TMP/match-complete.phg`: same enum, all three arms present (`Attack`/`Heal`/`Defend()`).

```
=== CHECK ===
OK (type-checks clean)
exit=0
=== RUN ===
defends
exit=0
```

Confirms the rejections above are due to genuine non-exhaustiveness, not a spurious/unrelated error:
a complete match type-checks clean and runs correctly. (Note: zero-payload variant `Defend` must be
written `Defend()` in the pattern — bare `Defend =>` would silently be a catch-all binding, a known
documented footgun, but that is orthogonal to exhaustiveness enforcement, which here works.)

---

## Conclusion

Exhaustiveness checking for `match` is **fully enforced** across enum scrutinees, class/interface union
scrutinees, and primitive unions (catch-all required). The check is in `src/checker/matches.rs` and even
handles a finer case (variants covered "only by guarded arms" do not count toward exhaustiveness). No
fix needed — this rule is sound. Severity: **none**.
