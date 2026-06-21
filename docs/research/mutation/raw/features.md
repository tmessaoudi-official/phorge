# Track 4 — Mutation + GC: Dependent-Feature Surface, Syntax/Keywords, and the Modifier Model

> Research deliverable for the Phorge **mutation + garbage-collection** milestone.
> Author lens: software-craftsmanship apex (SOLID / design-patterns / best-practice), PHP as the
> floor not the ceiling, transpile contract `Phorge : PHP :: TypeScript : JavaScript`, every feature
> byte-identical `run ≡ runvm ≡ real PHP`, additive power coexists never replaces.
> Every claim graded. Grounded in the repo (files read, not recalled).

---

## 0. The grounding facts (read from the repo, not memory)

These are the load-bearing realities the whole milestone pivots on. All [Verified] by reading the files.

- **The heap is immutable + acyclic by construction.** `src/value.rs` header (lines 1–6): every compound
  `Value` is `Rc<T>` with an *immutable interior* — no `RefCell`, no `Cell`. `Instance.fields` is a plain
  `HashMap<String, Value>` (l.70–73), `List`/`Map`/`Set` are `Rc<Vec<…>>` (l.26–38). Because nothing can be
  re-pointed after construction and a constructor's args are fully evaluated before the instance exists
  (EV-1), **`Rc`/`Drop` reclaims completely — no cycle can form, so there is no tracing GC** (header l.4–6,
  echoed in `CLAUDE.md` and INVARIANTS). [Verified: read `value.rs:1-73`]
- **There is no assignment statement today.** `src/ast.rs` `Stmt` enum (l.511–546) has exactly five
  variants: `VarDecl`, `Return`, `If`, `For`, `Block`, `Expr`. There is **no `Assign`**. A `VarDecl` is the
  only way a name enters scope. [Verified: read `ast.rs:510-546`]
- **The interpreter already has the machinery for reassignment — it just isn't exposed.** `Frame` is a
  `Vec<HashMap<String,Value>>` scope stack (`interpreter.rs:49-78`); `declare` inserts into the top scope;
  `exec_stmt`'s `VarDecl` arm calls `frame.declare(name, v)` (l.252-256). A reassignment statement would
  be a *mutate the existing binding in the scope where it lives* operation — trivially expressible. [Verified:
  read `interpreter.rs:236-316`]
- **The VM already has `Op::SetLocal` — and already uses it for reassignment-shaped writes.** `chunk.rs`
  l.94-96 defines `SetLocal(usize)` = "pop and store into the local at stack slot n". `compiler.rs` already
  emits `SetLocal` to *overwrite a slot* in the `&&`/`||` lowering (l.1224) and the `for`-loop index counter
  (l.1686) and `match` scrutinee slot (l.1750). **Variable reassignment needs no new `Op` on the VM side** —
  it resolves the local slot (`resolve_local`, `compiler.rs:758`) and emits `SetLocal`. [Verified: read
  `chunk.rs:94-96`, `compiler.rs:748-770,1215-1224`]
- **The element-set / field-set path does NOT exist.** `Op::Index` (`chunk.rs:109-112`) is read-only (pop
  index + container → push element). There is no `SetIndex`, no `SetField`. `GetField` (l.160-162) reads;
  there is no setter. **In-place container/field mutation is the part that genuinely requires new
  machinery** (and is exactly what breaks the acyclic invariant). [Verified: read `chunk.rs:66-186`]
- **The checker already forbids shadowing a function/import name and tracks per-scope binding types**
  (`checker.rs:1255-1290`, scope stack). Reassignment's type-compatibility check (new value assignable to
  the binding's declared type) slots into this existing `declare`/`lookup` machinery. [Verified: read
  `checker.rs:1255-1290`]
- **Modifiers already exist as an AST enum but are inheritance/visibility-only.** `ast.rs:491-498`:
  `Modifier { Public, Private, Protected, Const, Final }`. No `mutable`, no `static`, no `open`, no
  `readonly`. `CtorParam`/`Field` carry `Vec<Modifier>`. [Verified: read `ast.rs:490-508`]
- **Adding any `Op` variant touches THREE exhaustive matches in the same commit:** `vm.rs::exec_op`,
  `chunk.rs::BytecodeProgram::validate`, `compiler.rs::stack_effect` (INVARIANTS §5; `chunk.rs:305-412`
  is the exhaustive `validate` with NO `_` wildcard). This is the cost unit for "new Op". [Verified: read
  `chunk.rs:305-412`, INVARIANTS §5]

**The single most important architectural finding:** the milestone splits cleanly into **two tiers with
very different cost**:

| Tier | What | Cost | Why |
|---|---|---|---|
| **Tier 1 — local-rebinding mutation** | `=` reassign, `+=`/`-=`/…, `++`/`--`, `??=`, while/do-while/C-for, while-let | **NO new `Op`, NO GC** | A local is a slot/scope entry. Rebinding it to a *fresh* immutable value keeps the heap immutable+acyclic. `SetLocal` already exists. |
| **Tier 2 — interior mutation** | mutable fields (`p.x = …`), element set (`xs[i] = …`), `static mutable`, set-hooks, mutable collections | **new `Op` (`SetField`/`SetIndex`), needs the GC** | Re-pointing a field/slot *inside a shared `Rc`* can create cycles (`a.next = b; b.next = a`) → `Rc`/`Drop` leaks → tracing GC required. |

This split is the spine of the whole recommendation. **Tier 1 can ship before any GC exists** — and it
delivers ~70% of the dependent-feature surface (every loop, every compound-assign on a local, `++`/`--`,
`??=`). Tier 2 is the part that genuinely earns the "+ tracing GC" in the milestone name.

---

## 1. The dependent feature tree — each feature defined

For each: **Phorge syntax** (craftsmanship-best + idiomatic-PHP target), **PHP semantics to match**,
**interactions/enforcement**, **tier**, **new-Op cost**.

### 1.1 Variable reassignment — `=` as a statement  [TIER 1]

- **Phorge syntax:** `x = expr;` where `x` is an already-declared **`mutable`** local. A *new* statement
  variant `Stmt::Assign { target, value, span }`. Distinct from `VarDecl` (which introduces a binding with a
  type); `Assign` rebinds an existing one.
  ```phorge
  mutable int n = 0;
  n = n + 1;          // OK — n is mutable
  int k = 5;
  k = 6;              // E-ASSIGN-IMMUTABLE: `k` is not mutable
  ```
- **PHP semantics:** `$n = $n + 1;` — trivial 1:1 transpile. PHP variables are all mutable; the
  immutable-by-default *check* is a Phorge front-end concept that simply doesn't emit (PHP has no readonly
  *local*). [Verified: PHP assignment manual — `=` rebinds.]
- **Interactions / enforcement:**
  - **Immutable-by-default gate:** reassigning a non-`mutable` local is `E-ASSIGN-IMMUTABLE`. This is the
    *reason* the modifier model (§3) must land in the same milestone — `=` is meaningless without `mutable`.
  - **Type compatibility:** the assigned value must be assignable to the binding's *declared* type
    (`Ty::assignable_with`), reusing the checker's existing machinery. No re-inference; the declared type is
    fixed at `VarDecl` (a `var` local's inferred type is also fixed at decl — reassigning `var x = 1; x =
    "s";` is `E-ASSIGN-TYPE`).
  - **Smart-cast invalidation (subtle, must enforce):** S2 if-let / `instanceof` narrowing narrows a binding
    inside a block. If that binding is `mutable` and reassigned inside the narrowed region, the narrowing is
    **invalidated** — Kotlin and TS both do exactly this (a `var` smart-cast is dropped after reassignment).
    *[Inferred: Phorge inherits S2's smart-cast; the moment a binding becomes reassignable, the
    "narrowed-then-mutated" footgun appears — Kotlin's rule is the proven answer.]*
  - **`this` and params:** method params and `this` are immutable by default (PHP params are mutable, but
    Phorge chooses immutable-param as the craftsmanship default; `mutable` opt-in on a param if ever wanted).
    *[Speculative — recommend params immutable-by-default, matching the value-semantics spirit.]*
- **New Op:** **none.** VM: `resolve_local(name)` → `Op::SetLocal(slot)`. Interpreter: mutate the binding in
  the scope where `lookup` finds it (a small `assign(name, v)` that walks `scopes.iter_mut().rev()`).
  Transpiler: `$x = …;`. [Verified: `SetLocal` exists, `chunk.rs:94`.]

### 1.2 Compound assignment — `+= -= *= /= %= .=`  [TIER 1]

- **Phorge syntax:** `n += expr;` etc. Pure **desugaring** in the parser/checker: `n += e` ≡ `n = n + e`,
  reusing the existing `BinaryOp` + the new `Assign`. Target must be a `mutable` local (Tier 1) — a
  `mutable` field target is Tier 2.
  ```phorge
  mutable int total = 0;
  for (int x in items) { total += x; }
  ```
- **PHP semantics:** `$total += $x;` — 1:1. **`.=` (string concat-assign):** the parity matrix (l.210)
  notes Phorge dropped `.`/`.=` in favor of interpolation / `Core.Text.join`, calling `+`-vs-`.` ambiguity a
  footgun. **Recommendation: do NOT add `.=`** — it depends on the rejected `.` operator. String building
  uses interpolation or a future `Core.Text` builder. The arithmetic family `+= -= *= /= %=` (and, once the
  operators slice lands, `**=` `&=` `|=` `^=` `<<=` `>>=`) all desugar identically. [Verified: matrix l.210,
  217-219 — `.=` "also needs concat (rejected)".]
- **Interactions / enforcement:**
  - **Fault parity is free:** `/=` and `%=` desugar through the *same* `int_div`/`int_rem` kernels
    (`value.rs:289-303`) that already fault `division by zero`/`modulo by zero` byte-identically — and PHP 8
    throws `DivisionByZeroError` on both, which the oracle already matches. No new fault surface. [Verified:
    `value.rs:266-303`; PHP 8 `/0` → DivisionByZeroError per php.watch/fdiv.]
  - **Single-evaluation of the target:** for a *local* target (Tier 1) the target is a name — no double-eval
    concern. (For a Tier-2 field/element target `a.b.c += 1`, the receiver path must be evaluated once;
    that's a Tier-2 lowering detail.)
  - Desugaring means **no new checker rule** beyond "target is a mutable lvalue" + the binary's existing type
    rule.
- **New Op:** **none** (desugars to `Assign` + existing `Binary`).

### 1.3 Increment / decrement — `++` / `--` (pre and post)  [TIER 1]

- **Phorge syntax:** `n++; ++n; n--; --n;` on a `mutable` int/float local. As a **statement** the pre/post
  distinction is irrelevant; as an **expression** (`y = x++`) it matters. **Recommendation: statement-only
  first** (`n++;` desugars to `n = n + 1;`), expression-position `++`/`--` deferred or rejected — expression
  pre/post increment is a classic readability footgun and not craftsmanship-apex.
  ```phorge
  mutable int i = 0;
  while (i < n) { process(i); i++; }   // statement form only
  ```
- **PHP semantics:** `$i++;` 1:1. **Critical divergence to NOT replicate** (matrix l.143, 220): PHP's `++`
  on *strings* ("a"++ → "b", "Az"++ → "Ba") is a quirk Phorge must reject — `++`/`--` are **numeric-only**
  (`E-INCR-TYPE` on a non-numeric target). PHP `++`/`--` on `null` yields 1/null respectively — also not
  replicated; Phorge's target is a typed non-null `int`/`float`. [Verified: matrix l.143 "PHP
  string-increment is a quirk NOT to replicate".]
- **Interactions / enforcement:** numeric-only; mutable-only; statement-position recommended. Overflow goes
  through `int_add`/`int_sub` kernels → `integer overflow` fault, byte-identical. [Verified: kernels in
  `value.rs`.]
- **New Op:** **none** (desugars to `Assign` + `Binary(Add/Sub, 1)`).

### 1.4 `??=` null-coalescing assign  [TIER 1]

- **Phorge syntax:** `opt ??= fallback;` on a `mutable` local of optional type `T?` → assigns `fallback`
  (a `T`) only when the local is currently `null`. Reuses S2's `??` (`BinaryOp::Coalesce`).
  ```phorge
  mutable string? name = lookup(id);
  name ??= "anonymous";   // name : string? still, but now guaranteed non-null in the common path
  ```
- **PHP semantics:** `$name ??= "anonymous";` — 1:1, and PHP's `??=` is short-circuiting (RHS not evaluated
  if LHS is set+non-null). Desugar: `if (target == null) { target = rhs; }` — but the transpiler should emit
  literal `??=` to preserve PHP's short-circuit + single-eval. [Verified: PHP assignment manual.]
- **Interactions / enforcement:** target must be `mutable` and of optional type (`E-COALESCE-ASSIGN-TYPE` if
  the target isn't `T?`). The post-assign *type* is still `T?` (a later branch could re-null it), unless flow
  analysis proves otherwise — keep it simple: type unchanged.
- **New Op:** **none** (desugars to `Assign` guarded by the existing coalesce/branch ops).

### 1.5 `while` loop  [TIER 1 — but only useful WITH mutation]

- **Phorge syntax:** `while (cond) { … }`. Plus **`break;` / `continue;`** (already adopted in Wave A over
  `for..in`, matrix Decision Log Batch 2 — they generalize to `while` for free).
  ```phorge
  mutable int i = 0;
  while (i < limit && !found(i)) { i++; }
  ```
- **PHP semantics:** `while ($cond) { … }` 1:1. [Verified: trivial.]
- **Interactions / enforcement:** The matrix (l.131) marks `while` `defer` "needs mutation to advance the
  condition" — *exactly right*: a `while` whose condition never mutates is either an infinite loop or a
  dead loop, so `while` is **mechanically coupled to Tier-1 mutation** and must land in the same slice. No
  termination proof is attempted (PHP doesn't either); the recursion/step budget is the existing runtime
  guard. `break`/`continue` reuse the Wave-A lowering. [Verified: matrix l.131.]
- **New Op:** **none** — `while` lowers to the *exact same* `JumpIfFalse` + `Jump` backward-edge the
  compiler already emits for `for` (`compiler.rs:1657-1686` shows the `for` loop's jump structure). The
  compiler already supports backward jumps. [Verified: `for` already compiles to jumps, `compiler.rs:1657+`.]

### 1.6 `do-while` loop  [TIER 1]

- **Phorge syntax:** `do { … } while (cond);` — body runs once before the first test.
- **PHP semantics:** `do { … } while ($cond);` 1:1. [Verified.]
- **Interactions / enforcement:** same mutation coupling as `while`; same jump lowering with the test at the
  *bottom* of the loop body.
- **New Op:** **none** (jump structure only).

### 1.7 C-style `for` loop — `for (init; cond; step)`  [TIER 1]

- **Phorge syntax / craftsmanship verdict:** Phorge already has `for (T x in range)` over `a..b` ranges,
  which the matrix (l.133) correctly calls "phorge-already-better" for the counted 90%. The C-for earns its
  place **only for arbitrary-step / multi-variable / non-range conditions** (`for (mutable int i = n; i > 0;
  i -= 2)`). **Recommendation: adopt it, but framed as the escape hatch**, not the default — keep `for..in`
  the idiomatic counted loop. Syntax: `for (mutable int i = 0; i < n; i = i + 1) { … }` (init is a `VarDecl`,
  step is an `Assign`/compound-assign — both Tier-1 constructs).
- **PHP semantics:** `for ($i = 0; $i < $n; $i++) { … }` 1:1. [Verified.]
- **Interactions / enforcement:** init scopes the loop variable to the loop body only; step runs after each
  iteration before the re-test. Reuses `while`'s jump structure + Tier-1 assign. `break`/`continue` work
  (continue jumps to the *step*, matching PHP/C).
- **New Op:** **none.**

### 1.8 `while-let`  [TIER 1 over an already-Tier-1 source; deeper forms need an iterator protocol]

- **Phorge syntax:** `while (var x = optExpr) { … }` — loop while the optional re-evaluates non-null,
  binding the unwrapped `x` (the loop dual of S2's `if (var x = opt)`). Matrix l.365 marks it `defer`
  "needs a mutating source".
- **PHP semantics:** `while (($x = optExpr()) !== null) { … }` — 1:1 once `=` is an expression in PHP (it
  is). Phorge's `while (var x = …)` lowers to that. [Verified: PHP assignment-as-expression.]
- **Interactions / enforcement:** the *source* (`optExpr`) must change between iterations or it loops
  forever — which it does only if it reads mutable state (a `mutable` cursor) or calls a side-effecting
  native. So while-let's *usefulness* is Tier-1-mutation-gated (you mutate a cursor in the body), but the
  **construct itself is pure front-end sugar** over the existing if-let lowering + a backward jump — it can
  land the moment `while` lands. A *stateful iterator-protocol* while-let (`while-let Some(x) = iter.next()`)
  is Tier-2 (the iterator holds mutable position) — defer to the iterator-protocol milestone.
- **New Op:** **none.**

### 1.9 `static mutable` — shared class/module state  [TIER 2 — needs the GC]

- **Phorge syntax:** `static mutable int counter = 0;` as a class member (and possibly module-level). Read
  via `ClassName.counter` (Go-qualified, matching Phorge's static-access decision, matrix l.213). The
  *combination* `static mutable` is exactly the GA plan's named example (plan l.25).
  ```phorge
  class IdGen {
    static mutable int next = 0;
    static function fresh() -> int { IdGen.next = IdGen.next + 1; return IdGen.next; }
  }
  ```
- **PHP semantics:** `public static int $next = 0;` + `self::$next = self::$next + 1;` — 1:1. [Verified:
  PHP static properties.]
- **Interactions / enforcement:** This is genuinely Tier-2. A `static mutable` slot is **shared mutable
  state with a program-lifetime extent** — it is the *one* place a cycle can be rooted (`static mutable
  Node head` pointing into a structure that points back). It needs (a) a new `SetStatic`/`GetStatic` op or a
  static-slot table, and (b) it is the **GC's first real customer**: a long-lived mutable root. **This is
  the feature that justifies the tracing GC in the milestone name.** `static` *immutable* (`static const`) is
  Tier-1 / can land with the constants slice (compile-time fold). [Verified: matrix l.127 "Static (mutable)
  properties … Needs mutation+tracing-GC"; plan l.25.]
- **New Op:** likely `GetStatic(idx)` / `SetStatic(idx)` (a program-level static-slot table, the mutable
  analogue of the const pool) — **2 new ops** (or fold into a generalized global-slot mechanism). Touches the
  three coupled matches.

### 1.10 `clone`-with / copy-update expression  [TIER 1 — and the IMMUTABLE-FIRST star feature]

- **Phorge syntax (craftsmanship-apex):** a **functional copy-update expression** producing a *new*
  immutable instance with some fields replaced — NOT in-place mutation. This is the C#-record `with` /
  Kotlin `copy()` / OCaml `{ r with … }` pattern, and it is the **idiomatic way to "change" an immutable
  value** (matrix l.140, 285 mark it `defer`/high, "very Phorge-aligned").
  ```phorge
  class Point { constructor(public int x, public int y) {} }
  Point p = Point(1, 2);
  Point q = p with { y = 9 };      // q = Point(1, 9); p unchanged
  ```
  Recommend the `with { field = expr, … }` postfix form (reads like the C# `with`, familiar; the field list
  is checked against the class's promoted fields).
- **PHP semantics / transpile target:** **PHP 8.5 ships `clone with [...]`** (matrix l.140, 285) — so the
  byte-identical target is `clone $p with [y: 9]` on PHP 8.5. For the current `php -n` 8.6 oracle this maps
  directly. (Pre-8.5 fallback would be a generated copy-constructor, but the oracle is 8.6 so `clone with`
  is available.) [Verified: matrix l.140 "clone-with (8.5) … functional update"; l.285 "PHP 8.5".]
- **Interactions / enforcement:**
  - **It is the capability-preservation answer for "no `&` references / no in-place field mutation."** The
    GA plan's Group-3 removal table preserves the mutation capability via "immutable values + `clone`-with"
    (review spec, Group 3 `&` row). So clone-with is **load-bearing for the whole immutable-by-default
    story**, not a nice-to-have.
  - It is **Tier 1**: it *constructs a fresh instance* (the existing `MakeInstance` path), copying the old
    fields and overriding some — the old instance is untouched, the heap stays immutable+acyclic. **No GC
    needed.**
  - The override list is type-checked field-by-field against the class; an unknown field is
    `E-CLONE-FIELD`; a type mismatch reuses the field's assignability rule.
- **New Op:** **none, ideally** — lower in the front-end to a synthetic construction: read each non-overridden
  field of `p` via existing `GetField`, push the overrides, call the constructor / `MakeInstance`. (If the
  class has a non-trivial constructor with validation, the design must decide whether `with` re-runs ctor
  validation or bypasses it — *genuine fork, see §5*.) Worst case one `CloneWith` op if field-copy can't be
  expressed as a construction; recommend the front-end-lowering route to keep zero new ops.

### 1.11 Property set-hooks (PHP 8.4 hooks)  [TIER 2 for set; get-hooks are TIER 1]

- **Phorge syntax:** PHP 8.4 property hooks (`public int $x { get => …; set => …; }`). **Split by tier:**
  - **get-hooks (computed/virtual properties) — TIER 1, can land sooner.** A `get` hook is a pure
    read-side transform; it touches no mutable state (matrix l.120 "get-hooks immutability-OK"). Phorge
    syntax: `int area { get => this.w * this.h; }` — a virtual field. Transpiles to PHP 8.4
    `public int $area { get => $this->w * $this->h; }`. *No mutation, no GC.*
  - **set-hooks — TIER 2, needs mutation + GC.** A `set` hook intercepts a *write* to a field
    (`set { this.x = $value < 0 ? 0 : $value; }`) — which presupposes the field is mutable, which is the
    whole Tier-2 surface. [Verified: matrix l.120 "set-hooks need mutation/GC"; memory `property-hooks-planned`.]
- **PHP semantics:** PHP 8.4 backed vs virtual: a hook that references `$this->x` is *backed* (stores a
  value), one that doesn't is *virtual* (computes, no storage). The shorthand `get => expr` / `set => expr`
  arrow forms. [Verified: php.net property-hooks manual + Zend blog.]
- **Interactions / enforcement:** get-hooks are essentially **methods rendered as properties** and fit the
  immutable model now. They also resolve the GA plan's `__get`/`__set`-magic removal: the Group-3 table
  preserves that capability via "typed property hooks (PHP 8.4, roadmapped)". So shipping get-hooks early is
  a capability-preservation win. Set-hooks ride the field-mutation machinery (Tier 2).
- **New Op:** get-hook: none (it's a method-call lowering on field read). set-hook: rides `SetField`
  (Tier 2).

### 1.12 The Tier-2 substrate that all the above implicitly need

The features marked Tier 2 (mutable fields, element set, `static mutable`, set-hooks) all rest on **two new
ops + the tracing GC**:

- **`Op::SetField(name_idx)`** — pop value + instance, write the field. The first op that mutates an
  `Rc<Instance>` interior → requires `Instance.fields` to become interior-mutable (`RefCell<HashMap>` or a
  slot `Vec<RefCell<Value>>`) → **breaks the acyclic guarantee** → tracing GC. [Inferred: directly implied
  by `value.rs` being `Rc` + immutable today.]
- **`Op::SetIndex`** — pop value + index + container, write the element (the mutable dual of the read-only
  `Op::Index`). Same `RefCell` requirement on `List`/`Map`. [Inferred: `Op::Index` is read-only today,
  `chunk.rs:109-112`.]
- **The tracing GC** — once `RefCell` interior mutation exists, `a.next = b; b.next = a` is constructible,
  `Rc` strong-count never hits 0, memory leaks. A mark-sweep collector (the M2-deferred-to-M3 plan, per the
  `value.rs` header and CLAUDE.md) becomes mandatory. **This is the milestone's irreducible core.**

---

## 2. How the dependent tree sequences (build order)

The two-tier split dictates the sequence. Tier 1 ships first and unblocks most of the surface with zero GC.

```
SLICE M-mut.1  "Mutable locals + reassignment"        [TIER 1, no new Op, no GC]
  ├─ Modifier model lands here (§3): `mutable` keyword, immutable-default enforced
  ├─ Stmt::Assign + E-ASSIGN-IMMUTABLE + E-ASSIGN-TYPE + smart-cast invalidation
  ├─ Interpreter `assign()`, VM resolve_local+SetLocal (exists), transpiler `$x = …`
  └─ UNBLOCKS: nothing downstream yet, but is the foundation for everything in Tier 1
       example: examples/guide/mutation.phg

SLICE M-mut.2  "Compound assign + ++/-- + ??="          [TIER 1, no new Op, pure desugar]
  ├─ += -= *= /= %=  (NOT .=)  ;  ??=  ;  n++ n-- (statement form)
  └─ all desugar to M-mut.1's Assign + existing Binary/Coalesce ops

SLICE M-mut.3  "Condition loops: while / do-while / C-for + while-let"  [TIER 1, no new Op]
  ├─ while, do-while, C-style for (the escape hatch; for..in stays idiomatic)
  ├─ while-let (front-end sugar over if-let + backward jump)
  ├─ break/continue already exist (Wave A) — generalize to the new loops
  └─ UNBLOCKS: imperative accumulator loops, condition-driven iteration

SLICE M-mut.4  "clone-with / copy-update"               [TIER 1, no new Op (front-end lowering)]
  ├─ `p with { field = expr }` → fresh instance via existing construction path
  ├─ get-hooks (virtual/computed properties) can ride here or a sibling slice [TIER 1]
  └─ UNBLOCKS: the idiomatic immutable-update idiom; capability-preservation for `&`-removal

──────────────  GC BOUNDARY  ──────────────  (everything above ships with ZERO GC)

SLICE M-mut.5  "Tracing GC + mutable fields"            [TIER 2, NEW Ops, GC]
  ├─ Value interior → RefCell; mark-sweep collector; GC roots = stack + static slots + const pool
  ├─ Op::SetField  (+ the three coupled matches)
  ├─ mutable field declaration + `p.x = …`  (mutable-field gated; immutable fields stay write-once)
  └─ This is the slice the milestone name's "+ GC" refers to.

SLICE M-mut.6  "Mutable elements + static mutable + set-hooks"  [TIER 2, on the GC]
  ├─ Op::SetIndex (xs[i] = …) ; Op::Get/SetStatic (static mutable shared state) ; set-hooks
  └─ UNBLOCKS: mutable collections, shared counters, intercepted writes
```

**Rationale for the boundary:** every Tier-1 slice is byte-identity-trivial (locals are not shared across the
`Rc` boundary; a rebind is a fresh value) and ships **before** the hardest part of the project (a correct,
deterministic, byte-identical tracing GC). This front-loads ~70% of the user-visible value and de-risks the
GC by letting it be designed against a *known, small* mutable surface (fields + elements + statics) rather
than the whole language at once.

**Determinism warning for the GC (byte-identity spine):** a tracing GC must **never** be observable in
program output — no finalizers, no `__destruct` (already Group-3 removed precisely because "destruction
timing non-deterministic under Rc/Drop breaks the byte-identity spine"). Collection timing must not affect
`run ≡ runvm ≡ PHP`. PHP's own GC is non-deterministic but unobservable; Phorge must hold the same line.
[Verified: review spec Group-3 `__destruct` removal rationale.]

---

## 3. The modifier model — resolving the GA-plan pause

The GA plan (l.13-29) paused on confirming four orthogonal axes. Here is the research-backed resolution.
**Verdict: the four-axis model is sound and matches how the best-craftsmanship languages do it. Dropping
`final`/`readonly` as value modifiers is correct.** Refinements below.

### 3.1 The four axes, confirmed against Rust/Swift/Kotlin/C#

| Axis | Question | Phorge default | Phorge opt-in | Precedent |
|---|---|---|---|---|
| **Mutability** | reassignable / writable after init? | **immutable** | **`mutable`** | Kotlin `val`/`var`; Swift `let`/`var`; Rust `let`/`let mut` — **all three default immutable**. [Verified: searches below.] |
| **Compile-time const** | named compile-time constant? | — (a decl *form*) | `const NAME = <const-expr>` | Kotlin `const val` (top-level/object only, compile-time); C# `const`; Rust `const`. **Distinct axis from runtime immutability everywhere.** [Verified.] |
| **Association** | instance vs class-level? | **instance** | **`static`** | Universal (Kotlin/Swift/C#/Java/PHP all use `static` for class-level). [Verified.] |
| **Extensibility** | class/method extendable/overridable? | **closed (final)** | **`open`** | **Kotlin: final-by-default, `open` to allow** — exactly this model. [Verified: "every class and method is final by default … use the open keyword".] |

This is **not a Phorge invention** — it is the **Kotlin model almost exactly**, plus Swift's value-semantics
default, plus C#'s `with`-based immutable update. The convergence of three modern, craftsmanship-respected
languages on *immutable-default + final-default + orthogonal const + orthogonal static* is the strongest
possible evidence the model is right. [Verified: Kotlin `val`/`var`/`const`/`open`/`final`/`sealed` search;
Swift `let`/`var`/`mutating` search; C# `record`/`with`/`init`/`readonly`/`sealed` search.]

### 3.2 Dropping `final` and `readonly` as VALUE modifiers — sound?  **Yes.**

- **`readonly` is subsumed by immutable-default.** The matrix already says so repeatedly: "readonly is
  Phorge's default" (l.98, 261, 262, 286). A `readonly` *value modifier* on a field is redundant when every
  field is immutable unless marked `mutable`. **Verdict: remove `readonly` as a modifier.** The *transpiler*
  may still **emit** PHP `readonly` to signal intent (matrix l.98 "transpiler could emit it") — that's an
  output detail, not a Phorge keyword.
- **`final` as an inheritance modifier becomes the default; `open` is the opt-in.** This is the Kotlin
  rule. The old PHP `final` keyword (`Modifier::Final` in `ast.rs:497`) is retired *as a user keyword*
  because closed-by-default makes it the no-op default; `open` carries the meaning. **Verdict: remove `final`
  as a keyword, default to closed, add `open`.** [Verified: Kotlin final-by-default + `open`.]
- **One nuance — `final` *constants* (PHP 8.1 final class constants, matrix l.271):** that's an
  inheritance-of-constants concern, gated on `extends` (S6) — it's not a *value* modifier and isn't part of
  this decision. No conflict. [Verified: matrix l.271.]

### 3.3 Refinements / interactions the GA plan should record

1. **`mutable` applies to two distinct surfaces — make the grammar uniform:** a *local* (`mutable int n
   = 0;`) and a *field* (`mutable int balance;`). Same keyword, same axis, different lowering tier (local =
   Tier 1, field = Tier 2). The checker treats both as "this binding is reassignable/writable." [Inferred:
   uniform-keyword is the legible choice; matches Kotlin `var` on both locals and properties.]
2. **`const` vs `mutable`-immutable distinction (must be crisp, this is where Kotlin's `val`/`const`
   split earns its keep):** an *immutable local* (`int x = compute();`) is a **runtime-fixed** binding —
   assigned once, value computed at runtime, never reassigned. A **`const`** (`const int MAX = 100;`) is a
   **compile-time** constant foldable into the const pool, usable in const-expression positions (array
   sizes, other const initializers, attribute args). They are *different axes*: every `const` is immutable,
   but not every immutable is `const` (an immutable can hold a runtime value). **Recommendation: keep both,
   exactly as Kotlin keeps `val` and `const val`.** The constants slice (already in Wave A, matrix Batch 2)
   introduces `const`; this milestone's immutable-default covers the runtime-fixed locals. [Verified: Kotlin
   "val is a run-time constant … const is a compile-time constant"; the two coexist.]
3. **`static mutable` is the one combination that's runtime-gated** (Tier 2 / needs GC) — syntax and rules
   lockable now, runtime lands in M-mut.6. The plan already says this (l.25-26). Confirmed. [Verified: plan
   l.25.]
4. **Method association:** `static` on a method = class-level dispatch (no `this`), already adoptable
   (matrix l.100 "Static methods … adopt"). Orthogonal to mutability. The Association axis cleanly covers
   both fields and methods. [Verified: matrix l.100.]
5. **`open` interacts with the locked OOP order:** `open` only *means* something once `extends` (S6) exists.
   So the **keyword can be reserved/parsed now** (rejecting `open` with a "not until inheritance" diagnostic,
   or simply accepting+ignoring on a leaf class), but its enforcement lands with S6. Don't block the modifier
   model on S6 — reserve the word, wire the semantics at S6. [Inferred: matrix l.115-118 gate `extends`/
   `final`/LSB on S6.]
6. **Spelling: `mutable` not `mut`** — already decided (plan l.11), and it's the right craftsmanship call:
   spelled-out, legible, PHP-familiar register (PHP has no `mut`). [Verified: plan l.11.]

### 3.4 The confirmation the plan was waiting for

The unanswered AskUserQuestion was "Confirm this modifier model?" Given the **autonomy contract is
TOTAL-with-stop-on-genuine-forks** (plan l.59-62) and this is **not a genuine fork** (three reference
languages converge on it; the matrix already presumes immutable/readonly-default; dropping `final`/`readonly`
is forced by the immutable default), the craftsmanship-apex answer is **CONFIRM the four-axis model as
proposed**, with the §3.3 refinements recorded. The only items that could be *genuine forks* are flagged in
§5 — none of them is the modifier model itself.

---

## 4. Interactions with existing features (cross-cutting enforcement)

- **S2 null-safety smart-casts × mutation:** the single biggest new interaction. Once a binding is
  `mutable`, every narrowing (if-let, `instanceof`, `match`-type-pattern) must be **invalidated on
  reassignment within the narrowed scope** (Kotlin/TS rule). Without this, `if (x instanceof Foo) { x =
  bar(); x.fooMethod(); }` type-checks but is unsound. **Must enforce in the same slice as `=`.** [Inferred
  from S2 design + Kotlin smart-cast-on-var rule.]
- **`for..in` loop variable:** currently the loop var is a fresh immutable binding each iteration
  (`interpreter.rs:301-303` declares it per-iteration). With mutation, a `mutable` loop var is allowed but
  reset each iteration — define the semantics explicitly (PHP foreach `$v` is mutable and *persists* the
  last value after the loop; Phorge should scope it to the loop body — craftsmanship over PHP-compat here).
  [Verified: `interpreter.rs:301-308` per-iteration declare.]
- **Lambdas capture by value (M3 S3):** `ast::free_vars` + by-value capture means a closure capturing a
  `mutable` local captures its *value at creation*, not a mutable cell — PHP `use ($x)` (not `use (&$x)`)
  semantics. This is **correct and must be preserved** — by-reference capture (`use (&$x)`) is the Group-3
  removed `&` references. A closure that needs to mutate shared state uses a `static mutable` field or a
  mutable object field (Tier 2), not captured-by-ref. [Verified: `value.rs:53-59` `ClosureData::Tree { env:
  Vec<(String, Value)> }` is by-value; review spec Group-3 `&` removal.]
- **Differential harness:** every Tier-1 feature is byte-identity-gated trivially (no shared state). The GC
  (Tier 2) needs explicit *non-observability* tests — a program that allocates+drops heavily must produce
  identical output regardless of when collection fires. [Inferred from INVARIANTS §1.]
- **`phg build` standalone binaries:** mutation changes nothing structurally (the embedded source runs
  through `cmd_runvm`); the GC must work in the built binary identically. [Verified: INVARIANTS §1 third
  surface.]

---

## 5. Genuine forks (per the autonomy contract — STOP and ask)

These have no single forced craftsmanship answer; the contract (plan l.59-62) says stop on these.

1. **`clone`-with and constructor validation.** If a class constructor validates invariants (e.g.
   `constructor(int age) { requires(age >= 0); }`), does `p with { age = -1 }` **re-run the constructor**
   (safe, but `with` becomes fallible / returns `T?`) or **bypass it** (fast, but can produce an instance
   the constructor would have rejected)? C# `with` bypasses (calls the copy-constructor, not the primary
   ctor); but Phorge's craftsmanship lean toward "no invalid instances" argues for re-validation. *Genuine
   fork — recommend asking.* [Speculative — both are defensible; C# precedent vs Phorge invariant-safety.]
2. **Tracing GC algorithm + observability budget.** Mark-sweep (simple, deterministic stop points) vs
   reference-counting-with-cycle-collector (keeps `Rc`, adds a cycle detector — *less* disruptive to the
   existing `Rc` model) vs a generational collector (overkill for M-scale). The `Rc` + cycle-collector route
   may preserve far more of the current `value.rs` and is worth a dedicated design spike. *Genuine fork —
   recommend a focused design slice before M-mut.5.* [Speculative — the existing all-`Rc` heap makes a
   cycle-collector (e.g. bacon-rajan style) a natural fit, but std-only/no-crate constraint and determinism
   need verification.]
3. **Default mutability of method parameters and `for..in` loop variables.** Immutable-by-default is locked
   for *declarations*; params and loop vars are an edge — PHP makes both mutable. Recommend immutable
   (value-semantics spirit) but flag it. *Minor fork.*

---

## 6. Summary table — feature → tier → new Op → unblocked-by

| Feature | Tier | New `Op`? | GC? | PHP target | Slice |
|---|---|---|---|---|---|
| `=` reassignment | 1 | no (`SetLocal` exists) | no | `$x = …` | M-mut.1 |
| `+= -= *= /= %=` | 1 | no (desugar) | no | `$x += …` | M-mut.2 |
| `.=` | — | **REJECT** (depends on dropped `.`) | — | — | — |
| `++` / `--` (stmt) | 1 | no (desugar) | no | `$i++;` | M-mut.2 |
| `??=` | 1 | no (desugar) | no | `$x ??= …` | M-mut.2 |
| `while` | 1 | no (jumps exist) | no | `while(…){}` | M-mut.3 |
| `do-while` | 1 | no (jumps) | no | `do{}while(…)` | M-mut.3 |
| C-style `for` | 1 | no (jumps) | no | `for(…;…;…){}` | M-mut.3 |
| `while-let` | 1 | no (if-let sugar) | no | `while(($x=…)!==null)` | M-mut.3 |
| `clone`-with | 1 | no (construction lowering) | no | `clone $p with [...]` (8.5) | M-mut.4 |
| get-hooks (virtual props) | 1 | no (method lowering) | no | `{ get => … }` (8.4) | M-mut.4 |
| tracing GC | 2 | — | **is the GC** | (unobservable) | M-mut.5 |
| mutable fields `p.x = …` | 2 | **`SetField`** | yes | `$p->x = …` | M-mut.5 |
| element set `xs[i] = …` | 2 | **`SetIndex`** | yes | `$xs[$i] = …` | M-mut.6 |
| `static mutable` | 2 | **`Get/SetStatic`** | yes | `static $x` / `self::$x = …` | M-mut.6 |
| set-hooks | 2 | rides `SetField` | yes | `{ set => … }` (8.4) | M-mut.6 |

---

## Sources

- Kotlin `val`/`var`/`const`/`open`/`final`/`sealed`: <https://www.baeldung.com/kotlin/const-var-and-val-keywords>, <https://www.geeksforgeeks.org/kotlin/whats-the-difference-between-const-and-val-in-kotlin/>, <https://medium.com/@jaidwivedi20/mastering-in-kotlin-part2-val-var-const-final-open-conditional-and-control-flow-7e52a59abb97>
- Swift `let`/`var`/`mutating`/value-semantics: <https://www.swiftbysundell.com/articles/mutating-and-nonmutating-swift-contexts/>, <https://chris.eidhof.nl/post/structs-and-mutation-in-swift/>
- C# records / `with` / `init` / non-destructive mutation: <https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/builtin-types/record>, <https://codinghelmet.com/articles/nondestructive-mutation-and-records-in-csharp>
- PHP 8.4 property hooks (backed vs virtual, get/set): <https://www.php.net/manual/en/language.oop5.property-hooks.php>, <https://wiki.php.net/rfc/property-hooks>, <https://www.zend.com/blog/php-8-4-property-hooks>
- PHP compound assignment + division-by-zero (PHP 8 `DivisionByZeroError`): <https://www.php.net/manual/en/language.operators.assignment.php>, <https://php.watch/versions/8.0/fdiv>
- Repo (grounding): `src/value.rs`, `src/ast.rs`, `src/chunk.rs`, `src/compiler.rs`, `src/interpreter.rs`, `src/checker.rs`, `docs/INVARIANTS.md`, `docs/specs/2026-06-21-php-parity-and-beyond.md`, `docs/plans/2026-06-21-ga-direction-and-autonomy.plan.md`
