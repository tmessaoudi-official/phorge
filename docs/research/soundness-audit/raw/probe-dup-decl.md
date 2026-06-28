# Soundness probe — duplicate field / method / param names

**Stage 2.** Rule under test: a class with two fields of the same name, or a function/method/constructor
with two parameters of the same name, MUST be rejected. (Each is a name that silently shadows itself —
a "declared but not enforced" uniqueness rule.)

Binary: `/stack/projects/phorj/target/release/phg` (prebuilt release, not rebuilt).
Probes under `$TMP/audit/` (scratchpad only — no repo files modified).

## Summary of verdicts

| Sub-rule | Verdict | Severity |
|---|---|---|
| Duplicate **method** (same name, identical param types) | **ENFORCED** (`E-OVERLOAD-DUPLICATE`) | — |
| Duplicate **constructor-promoted field** (`constructor(public int x, public int x)`) | **GAP** — runs, second wins | P1 |
| Duplicate **explicit field** (`mutable int x; mutable int x;`) | **GAP** — checks clean, runs | P1 |
| Explicit field **collides with** promoted field (`public int x; constructor(public int x)`) | **GAP** — runs | P1 |
| Duplicate **parameter** name (`function add(int a, int a)`) | **GAP** — checks clean, runs, second wins | P1 |

Methods are the only one of the family that is enforced. Fields (explicit, promoted, and the cross
collision) and parameters are all silently accepted — a declared-uniqueness rule that is not enforced.
Graded **P1**: not a type hole that lets provably-wrong code compute a wrong-typed value, but a soundness
rule the checker should guarantee and silently ignores; the duplicate just shadows itself (last
declaration / last argument wins), which is a footgun, not unsoundness. (If any duplicate could have a
*different type* the verdict would escalate — see "Type-divergence note" below.)

---

## Evidence

### 1. Duplicate method — ENFORCED (control / right-reason rejection)

Probe `$TMP/audit/dup-methods.phg`:
```
class C {
  constructor(public int x) {}
  function m(): int { return 1; }
  function m(): int { return 2; }
}
```
```
$ phg check dup-methods.phg
type error at 7:3: overloaded method `m` has two declarations with identical parameter types
  function m(): int { return 2; }
  ^
  [E-OVERLOAD-DUPLICATE]
  hint: each overload must differ in its parameter types
...
exit=1
```
Rejected for the RIGHT reason (`E-OVERLOAD-DUPLICATE`). Enforced.

### 2. Duplicate constructor-promoted field — GAP

Probe `$TMP/audit/dup-fields.phg`:
```
class C {
  constructor(public int x, public int x) {}
  function get(): int { return this.x; }
}
function main(): int { C c = new C(1, 2); Console.println("{c.get()}"); return 0; }
```
```
$ phg run dup-fields.phg
2
exit=0
```
Accepted and runs. Two `public int x` promoted params → one field; the **second argument (2) wins**.
Should be rejected (duplicate field/param name). **GAP.**

### 3. Duplicate explicit field — GAP

Probe `$TMP/audit/dup-fields2.phg` (first attempt used a non-`mutable` field and was rejected for the
UNRELATED reason `E-ASSIGN-IMMUTABLE`; re-run with `mutable` to isolate the duplicate):
```
class C {
  mutable int x;
  mutable int x;
  constructor(int a) { this.x = a; }
  function get(): int { return this.x; }
}
```
```
$ phg check dup-fields2.phg
OK (type-checks clean)
exit=0
$ phg run dup-fields2.phg
5
exit=0
```
Two identical field declarations type-check clean and run. **GAP.**

### 4. Explicit field collides with promoted field — GAP

Probe `$TMP/audit/dup-fields3.phg`:
```
class C {
  public int x;
  constructor(public int x) {}
  function get(): int { return this.x; }
}
function main(): int { C c = new C(7); Console.println("{c.get()}"); return 0; }
```
```
$ phg run dup-fields3.phg
7
exit=0
```
An explicit `public int x` and a promoted `public int x` collide silently. **GAP.**

### 5. Duplicate parameter name — GAP

Probe `$TMP/audit/dup-params.phg`:
```
function add(int a, int a): int { return a; }
function main(): int { Console.println("{add(3, 5)}"); return 0; }
```
```
$ phg check dup-params.phg
OK (type-checks clean)
exit=0
$ phg run dup-params.phg
5
exit=0
```
`add(3, 5)` returns **5** — the second `a` parameter silently shadows the first; the first argument is
unreachable. Type-checks clean. **GAP.**

---

## Type-divergence note (why these are P1 not P0, with the escalation condition)

The duplicates probed all share a type (`int`/`int`), so the only observable effect is shadow-last-wins.
The free-function param map (`collect.rs:914`) and the method param map (`:1337`) are
`f.params.iter().map(...).collect()` into a `Vec<Ty>` — a duplicate *name* with **different types**
(`function add(int a, string a)`) would still be accepted; whichever the body binds determines the
operand type. That is the boundary where a duplicate-name gap could become a type hole. It was not
re-probed here (out of this stage's two-name scope) but is flagged as the escalation path: if the
checker binds the FIRST param's type while the runtime binds the LAST argument, that is P0. Worth a
follow-up probe.

## Root cause / concrete fix

All four GAP variants share one cause: **fields, promoted fields, and parameters are accumulated into
maps/vecs with no uniqueness pass**, unlike methods (which DO get `E-OVERLOAD-DUPLICATE`) and interface
methods (which get a "duplicate method" check at `src/checker/collect.rs:112`).

File: **`src/checker/collect.rs`** — the class-collection loop (`ClassMember::Field` ~line 1278,
`ClassMember::Constructor`/promotion ~line 1299–1307), plus the free-fn param resolution (~line 914) and
the method param resolution (~line 1337).

Recommended fix:
1. **Fields + promotions:** maintain a `seen_fields: HashSet<String>` (or check
   `fields.contains_key`/`statics.contains_key` before `fields.insert` at `:1278` and before
   `promoted.push` at `:1299`). On a repeat, emit a new diagnostic, e.g. `E-DUP-FIELD`
   ("duplicate field `x` in class `C`"). Must cover all three collision shapes: field+field,
   promoted+promoted, field+promoted (check the union of explicit-field names and promoted-param names).
2. **Parameters:** before building the `Vec<Ty>` at `:914` (free fns), `:1337` (methods), and the ctor
   param loop at `:1289`, scan `params` for a repeated `p.name` and emit `E-DUP-PARAM`
   ("duplicate parameter `a`"). This is a single shared helper (`reject_dup_param_names(&params)`) that
   all three call sites can reuse.

A regression test belongs in `src/checker/tests/` (mirror the `overloading.rs` pattern that asserts on
`e.code`).
