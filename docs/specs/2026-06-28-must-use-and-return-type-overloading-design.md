# Must-Use Returns + Return-Type Overloading — Design

- **Date:** 2026-06-28
- **Status:** Designed — pending user review, then plan.
- **Milestone:** GA marathon (added 2026-06-28 alongside super/parent dispatch).
- **Backends:** front-end-only where possible; byte-identical `run ≡ runvm ≡ real PHP 8.5`.

## The insight

These are **two co-designed features**, not a bundle. Forbidding silent discard of a
non-`void`/`Empty` result means **every call site has a resolving type context** — which is
*exactly* the precondition that makes return-type overloading statically decidable. Must-use is the
enabler; return-type overloading is the payoff. Must-use also ships value on its own (catches
ignored `Result`, ignored computed values), so it lands first and standalone.

Sequencing: **Slice A (must-use) → Slice C (return-type overloading)**. (Slice B = super/parent
dispatch, separate spec.)

---

## Slice A — Must-use return values

### Rule

Any **expression used as a statement** whose value type is **not `void` and not `Empty`** is a hard
error (`E-UNUSED-VALUE`) unless it is explicitly discarded with the `discard` keyword. This covers
call results, `new` instances, and stray expressions (`x + 1;`) — the broadest, single-rule net
(decision: must-use scope = "any non-void/Empty expression-statement").

```phorj
compute();              // E-UNUSED-VALUE if compute(): int
discard compute();      // OK — deliberately thrown away
int x = compute();      // OK — used
log("hi");              // OK — log(...): void
doThing();              // OK — doThing(): Empty
```

### `discard` keyword

- New **contextual** statement keyword (like `var`/`when`/`as` — not reserved, so existing
  identifiers named `discard` are unaffected; only `discard <expr>;` in statement position is the
  new form). `discard` takes any expression, evaluates it for its side effects, and drops the value.
- For a **return-type-overloaded** callee, `discard` supplies *no* type context, so a selector is
  mandatory: `discard <int>f();` (see Slice C). Bare `discard f();` on a return-overloaded `f` is
  `E-OVERLOAD-NO-CONTEXT`.

### `void` / `Empty` are exempt

A `void` (no value) or `Empty` (unit value) result may be dropped silently — these are the only
discardable-by-default types. (Confirm `Empty`'s exact status during implementation; it is the unit
value type from Language-Evolution Phase 0.)

### Backend impact

- **Front-end only.** The checker raises `E-UNUSED-VALUE`; no runtime or `Op` change.
- **Transpile:** `discard e;` emits the PHP expression-statement `e;` (PHP discards naturally). A
  used result transpiles unchanged. No new helper, no runtime divergence.

### Breaking change

This is a **breaking reshape** (like the `this.field`-everywhere change): every existing call site
that drops a non-`void` return needs a `discard` (or a binding). A one-shot codemod inserts
`discard ` before offending expression-statements across `examples/`, fixtures, and inline test
programs. Land the checker rule + codemod in the same change; gate must stay byte-identical
afterward.

### New diagnostics (Slice A)

- `E-UNUSED-VALUE` — a non-`void`/`Empty` expression-statement is neither used nor `discard`-ed.
  Hint: "bind the result or prefix `discard`."

---

## Slice C — Return-type overloading

### What it adds

Overloads of one name may differ **only in return type** (identical parameter count + types), e.g.

```phorj
function parse(string s): int    { ... }
function parse(string s): bool   { ... }

int  a = parse("7");          // resolves to the : int overload
bool b = parse("yes");        // resolves to the : bool overload
discard <int>parse("7");      // explicit selector; value dropped
```

This **relaxes the current `E-OVERLOAD-RETURN` invariant** (today the checker *requires* an overload
set to share one return type; `compiler/program.rs` keeps the first overload's return meta). Return
types in a set may now diverge; resolution picks the member.

### Resolution = compile-time, shallow / direct-only

Param-overloads stay **runtime-dispatched by argument values** (unchanged). Return-overloads are
**resolved by the checker** from the *expected type* at the call site, propagated **only from a
directly-adjacent sink** — never through a constraint solver. The expected type is read from exactly
these **inferable sinks**:

1. **Typed binding** — `int x = f()` (a typed declaration; `var x = f()` is *inferred* and therefore has no context → error for a return-overload)
2. **Typed reassignment** — `x = f()` where `x`'s declared type is known
3. **Typed field write** — `this.count = f()` (field `count: int`)
4. **`return f()`** in a function/method/lambda with a declared return type
5. **Argument to a *non-overloaded* parameter of a single concrete type** — `h(f())` where
   `h(int p)` and `h` is not itself overloaded

**Everything else requires an explicit `<type>f(...)` selector** (or it is a hard error): `discard`,
arithmetic/comparison operands (`f()+1`, `f()==0`), method receiver (`f().m()`), argument to an
**overloaded** callee, a union/optional/generic parameter, string interpolation `"{f()}"`, index
`xs[f()]`, and use as a first-class function value.

**Posture:** start minimal — widening the inferable set later is non-breaking; narrowing it is
breaking. When unsure, leave a sink in the `<type>`-required bucket.

### `<type>f(...)` — the overload selector

- New **prefix** form: an angle-bracketed type immediately before a call expression selects the
  overload whose return type is that type. It is an **overload selector, NOT a value cast** — it is
  semantically distinct from `as` (`x as T` → `T?` checked cast). Documentation must make this
  explicit, since `<int>f()` superficially resembles a C-style cast.
- Grammar: `<` cannot currently begin an operand (it is infix-only), so `<Type>callExpr` at
  operand position is a clean new production. **Nested generics hit the `>>` lexing trap**
  (`<List<int>>f()` ends in `>>` = the right-shift token); reuse the `> >` split the type-annotation
  parser already performs for `Box<List<int>>`.
- A `<type>` naming a return type **no overload of the callee has** → `E-OVERLOAD-SELECT-UNKNOWN`.

### Resolution rule under subtyping

The expected type `T` (from a sink or selector) selects the overload as follows:

1. The overload whose return type **exactly equals** `T` — if one exists, pick it.
2. Else the **unique** overload whose return type is **assignable to** `T` — pick it.
3. Else (zero or ≥2 candidates) → `E-OVERLOAD-AMBIGUOUS-RETURN`; the fix is an explicit `<type>`.

So `int x = f()` with `int`/`float` overloads is unique (float ⊀ int), but `Animal a = f()` with
`Dog`/`Cat` overloads (both `<: Animal`) is ambiguous → requires `<Dog>`/`<Cat>`.

When **both** a sink type and a `<type>` selector are present and **disagree**
(`string x = <int>f()`) → `E-OVERLOAD-SELECT-CONFLICT`.

### Dispatch model & PHP transpile

PHP has no overloading and cannot see the caller's expected type at runtime, so return-overloads
**cannot** dispatch at PHP runtime. They are resolved entirely by the checker and **name-mangled per
return type** for emission (reusing the established loader-side mangle/rewrite discipline):

- Each return-overload of `f` emits a distinct PHP function (e.g. `f__ret_int`, `f__ret_bool`); the
  resolved call site rewrites to the mangled name. Single-return-type names stay bare (so existing
  single-overload programs are byte-identical — no mangling on the common path).
- A set that **mixes** param-overloading and return-overloading: param dispatch stays runtime (the
  existing dispatcher), return dispatch is resolved statically first, then the param dispatcher runs
  within the chosen return-mangled function. Define precisely in the plan; this is the one genuinely
  new interaction.
- Interpreter + VM: resolution is done in the checker and threaded as the concrete target (the
  interpreter's `select_free_overload` / the VM dispatch table key on the resolved member), so
  `run ≡ runvm`. No new `Op`, no `Value` change.

### New diagnostics (Slice C)

- `E-OVERLOAD-NO-CONTEXT` — return-overloaded call in a non-inferable position without `<type>`.
- `E-OVERLOAD-AMBIGUOUS-RETURN` — context type matched by ≥2 (or 0) overloads.
- `E-OVERLOAD-SELECT-UNKNOWN` — `<type>` names a return type no overload has.
- `E-OVERLOAD-SELECT-CONFLICT` — sink type and `<type>` selector disagree.
- (Relax / repurpose the existing `E-OVERLOAD-RETURN`.)

All four self-document via `phg explain`.

---

## Interaction (why they are co-designed)

Must-use (Slice A) guarantees no return-overloaded result is ever silently dropped, so every such
call sits in a sink (Slice C cases 1–5) **or** carries a `<type>` selector **or** is a `discard
<type>f()`. There is no fourth case — which is what keeps return-overload resolution decidable and
the checker free of a constraint solver.

## Examples (ship with the features)

- `examples/guide/must-use.phg` — used vs `discard`, void/Empty exemption.
- `examples/guide/return-overloading.phg` — sink-resolved calls, `<type>` selector, `discard
  <type>f()`. Byte-identical on all three backends; round-tripped through real PHP 8.5.

## Out of scope / deferred

- Return-overloaded function as a first-class **value** (no context) — error; aligns with existing
  deferred fn-value limits.
- Widening the inferable sink set (index, interpolation, arithmetic operands) — non-breaking
  follow-ups if ergonomics demand.
- `discard` on a block/loop (statement-level only this slice).

## Test plan

- Checker: each new `E-*` code (positive + negative), the subtyping resolution rule, the
  sink/selector conflict, the `>>` nested-generic parse.
- Differential (`tests/differential.rs`): the two guide examples + targeted programs gating
  `run ≡ runvm` and the PHP-8.5 oracle (mangled-name emission round-trips).
- Codemod: the must-use migration leaves the full example/fixture corpus green and byte-identical.
