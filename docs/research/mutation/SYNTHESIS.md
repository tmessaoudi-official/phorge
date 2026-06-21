# Mutation + GC — Research Synthesis (decision-oriented)

> Author pass: 2026-06-21. Inputs: five research tracks (`raw/semantics.md`, `raw/gc.md`,
> `raw/byte-identity.md`, `raw/features.md`, `raw/vm-impact.md`) + a direct re-read of `src/value.rs`,
> `src/chunk.rs`, `src/ast.rs`, `docs/INVARIANTS.md`, the GA plan, and the parity spec. Filter:
> craftsmanship-apex; PHP is the floor; transpile contract `Phorge : PHP :: TS : JS`; the spine is
> `run ≡ runvm ≡ real PHP`, byte-identical. Every claim graded inline.
>
> **Read §1 and §3-Fork-1 first.** The tracks *disagree* on the spine decision, and that disagreement
> is the whole milestone.

---

## 1. Executive summary

**The single most consequential finding: the five tracks split into two camps on the spine question,
and the camp grounded in *executed real PHP* wins — which means a cycle-collecting GC IS required,
but only for the instance subset.**

- **Tracks 1 (semantics) + 2 (gc)** recommend **mutable value semantics for the *entire* heap,
  objects included** → no aliasing → no cycles → **no GC ever**. Elegant, citable (Hylo / mutable
  value semantics), and it would let the milestone drop its "+ GC" half. [Verified: the two tracks say
  exactly this — semantics.md §TL;DR, gc.md §0.]
- **Track 3 (byte-identity) ran the real PHP 8.6 oracle and proved the opposite is forced.** PHP
  objects are *handle/reference* types — `$p = $o; $p->v = 99;` makes `$o->v == 99` too
  (`obj_ref: o->v=99 p->v=99`, executed). PHP arrays are *value/COW* types — `$b = $a; $b[] = 4;`
  leaves `$a` untouched (`arr_copy: a=3 b=4`, executed). [**Verified**: Track 3 pasted the outputs of
  programs run against `/stack/tools/phpbrew/php/php-master/bin/php`, `php -n`, PHP 8.6.0-dev.]
- **Track 5 (vm-impact) independently reaches the same split** from the representation side: `make_mut`
  COW matches PHP *arrays* exactly but **diverges** from PHP *objects*, so instances need shared
  mutability. [Verified: vm-impact.md §3.]

**Resolution under the governing philosophy: the value/handle split is FORCED, not a fork.** Invariant
#1 (`docs/INVARIANTS.md` §1) requires byte-identity with **real PHP**, and the M7 PHP oracle
(`PHORGE_REQUIRE_PHP=1`) *fails the build* if it diverges. A Phorge program that aliases an object and
mutates it must print what PHP prints. Tracks 1+2's pure-value-semantics model would print something
PHP never prints — it is unshippable under the existing oracle. Tracks 1+2's elegance is real but it
answers a *different* language than the one the transpile contract pins Phorge to.

**Therefore:**
- `List` / `Map` / `Set` / `Bytes` → **value semantics, copy-on-write via `Rc::make_mut`**. Matches PHP
  arrays byte-for-byte, needs **no GC** (value types provably cannot form a cycle — assigning a list
  copies it; it can never refer back to a container that holds it). [Verified: byte-identity.md §2.11.]
- `Instance` (and `Enum` payloads that can hold instances) → **handle/reference semantics**, which
  *can* form a cycle (`a.next = b; b.next = a` — buildable in real PHP, `cycle_built: yes`) → `Rc`/`Drop`
  leaks → **a cycle collector is required, scoped to the instance subset only.** [Verified: byte-identity
  §2.11 ran the cycle; value.rs header anticipates exactly this.]

**Is tracing GC "actually needed"?** *A full tracing/mark-sweep arena: NO* (Track 2 disqualifies it —
the tree-walker has no scannable root set without `unsafe`, which Invariant #10 forbids). *A
cycle-collector layered on the existing `Rc`: YES, but narrowly* — only the cyclic instance subset, only
for memory correctness (never observable in output, because `__destruct` stays rejected), and it can be
**deferred to the last slice** because every value-type and local-rebinding feature ships before it.

The milestone's real shape: **~70% of the user-visible value (every loop, `+=`, `++`, `??=`, indexed
collection assign, `clone`-with) ships with ZERO GC.** The GC is one focused final slice for
shared-mutable object graphs.

---

## 2. The FORCED decisions (no developer call needed — invariants/contract decide them)

| # | Forced decision | Why it is forced | Grade |
|---|---|---|---|
| F1 | **`List`/`Map`/`Set`/`Bytes` = value semantics via `Rc::make_mut` COW.** | PHP arrays are COW value types (executed: `arr_copy`, `nested_arr`). `make_mut` is the exact std analog of PHP's refcount-keyed zval separation / Swift's `isKnownUniquelyReferenced`. Anything else diverges from the PHP oracle. | Verified (byte-identity §1-§2.3, vm-impact §3) |
| F2 | **`Instance` = handle/reference semantics (shared-mutable).** | PHP objects are handles (executed: `obj_ref`, `pass_handle`, `obj_arr_alias`). Value semantics for objects would print what PHP never prints → fails `PHORGE_REQUIRE_PHP=1`. | Verified (byte-identity §1) |
| F3 | **One mutation kernel in `value.rs`** (`list_set`/`list_push`/`map_set`/`set_field`), called by both backends — never hand-inlined. | Invariant #3 (arith/compare single-sourced) extended to mutation; re-inlining is the `Op::Neg` drift class. The aliasing-observability boundary is where the two backends silently drift. | Verified (INVARIANTS §3; vm-impact §7 ranks this P0) |
| F4 | **`eq_val` must become cycle-safe** (visited `Rc::ptr_eq` set) *before* any object→object mutation ships. | Today `eq_val` recurses unguarded (`value.rs`); a cyclic `==` overflows the native Rust stack — and the VM and tree-walker overflow at *different* depths (not frame-counted), a divergent crash that breaks `agree_err`. PHP's `==` is cycle-protected. **P0 prerequisite.** | Verified (byte-identity §2.10) |
| F5 | **No new loop opcodes.** `while`/`do-while`/C-`for` lower to existing `Jump`/`JumpIfFalse` + `SetLocal`. | The jump ops exist, are exhaustive in all three matches, and a backward target trivially passes `validate` (`target ≤ code_len`). Adding loop ops is redundant. | Verified (vm-impact §2.3; chunk.rs:350) |
| F6 | **Local reassignment reuses `Op::SetLocal`** — zero new Op. Interpreter gains a ~6-line `assign(name,v)` that overwrites the binding in the scope where `lookup` finds it (not a child-scope shadow). | `Op::SetLocal` already exists and is wired through all three matches; the compiler already emits it to overwrite slots (`&&`/`||`, `for`-index, `match`-scrutinee). | Verified (vm-impact §0; byte-identity §2.1) |
| F7 | **PHP `/=` and `%=` emission routes through `__phorge_div`/`__phorge_rem` helpers**, not naked PHP `$x /= e`. | M7 runtime-helper correctness model: naked PHP `/` is float-division, diverging from Phorge intdiv. Compound `/=`/`%=` desugar through the same fault-parity kernels. | Verified (vm-impact §6; transpile.rs:264-274) |
| F8 | **Loop keeps eval-once-materialize-then-iterate.** | This already gives PHP's "foreach iterates a copy" semantics for free (executed: `mut_test2` prints `1 2 3`, appended element not visited). Don't regress to live-buffer iteration. | Verified (byte-identity §2.4; interpreter.rs:297) |
| F9 | **Defaults are per-call** (never evaluate-once). | PHP forbids non-const defaults; evaluate-once + mutable = cross-call aliasing PHP has no concept of → unmatchable. Also removes the Python mutable-default footgun (philosophy: remove surprises). | Verified (byte-identity §2.5) |
| F10 | **Reject set, forced by prior decisions + parity:** `&` references, `foreach`-by-reference (`as &$v`), PHP string-`++`, `__clone`/`__get`/`__set`/`__destruct`, `===` on value types, mutable-evaluate-once defaults. | Each is an aliasing / non-determinism / coercion-footgun form the parity spec already rejects (Group 2/3) or that has no `make_mut`/byte-identical encoding. Capability preserved another way (per philosophy) — see §2-capabilities. | Verified (parity spec lines 165/172/176/183/229/461-462; byte-identity §3) |
| F11 | **GC, if any, is the `Rc`-cycle-collector family scoped to the instance arena — NOT a mark-sweep tracing arena.** | The tree-walker keeps call records on the native Rust stack (no enumerable root set); a tracing arena needs conservative stack scanning = `unsafe` = forbidden by Invariant #10. `Rc` reclamation is also the only model that makes weak-ref observability byte-identical across backends. | Verified (gc.md §3c, §5; ARCHITECTURE.md) |
| F12 | **GC must be observationally invisible — `__destruct`/finalizers stay rejected forever.** | Collection timing is non-deterministic (PHP collects on a 10k-root buffer fill); it is spine-safe *only* because nothing observable fires on reclamation. The project already removed `__destruct` for exactly this. | Verified (gc.md §5; parity spec line 462) |
| F13 | **Every mutation primitive ships with a two-binding "observe-after-mutate" differential example** in the PHP-gated `examples/**/*.phg` glob. | `agree`/`agree_err` compare the two Rust backends only — they can *both* alias a List wrongly and still agree with each other. Only the PHP oracle catches a value/handle slip; only a two-binding test exercises it. (Mirrors the `null-op scratch-slot` lesson.) | Verified (byte-identity §4.8, §5) |

**Capability-preservation map (per philosophy — removing a form preserves the power another way):**

| Removed form | Capability preserved via |
|---|---|
| `&` references / `foreach as &$v` | object handles (shared mutation where PHP would have it); index-mutating loop `a[i] = f(a[i])`; `Core.List.map` (shipped) |
| `__clone` / `__get`/`__set` | `clone with` (deterministic); typed property hooks (PHP 8.4, get-hooks ship early) |
| PHP string-`++` | numeric `++` only |
| mutable evaluate-once default | per-call fresh default (PHP-identical) |
| `readonly`/`final` modifiers | immutable-by-default; `open` opt-in (transpiler may still *emit* PHP `readonly` as intent) |

---

## 3. The GENUINE FORKS (developer's call — ordered by how much they gate everything else)

### Fork 1 — Object aliasing model: PHP-faithful shared-mutable, or value semantics for objects too? **[GATES THE ENTIRE MILESTONE]**

**Crisp statement:** PHP objects are reference types (executed: aliasing is observable). The transpile
contract + Invariant #1 force Phorge to match that *if objects can be aliased-then-mutated at all*. But
Tracks 1+2 argue Phorge should instead make objects *value* types (no aliasing), accept a deliberate,
documented divergence from PHP object semantics, and win a GC-free language. This is the one place the
research genuinely contradicts itself, and the answer reshapes every downstream slice.

| Option | What it means | Downstream blast radius |
|---|---|---|
| **(A) PHP-faithful handle semantics for objects** (Tracks 3+5) | `b = a` shares the instance cell; mutation visible through both; cycles possible. Instances become `Rc<RefCell<Instance>>`-equivalent. | **Forces the cycle-collector** (one final slice). `eq_val` must go cycle-safe (F4). Per-field-read `.borrow()` cost on instances (the M2 P5a hot path must be re-benched — Invariant #11). Enables `===` identity, doubly-linked lists, observers, `static mutable` graphs. **Byte-identical with PHP by construction.** |
| **(B) Value semantics for objects too** (Tracks 1+2) | `b = a` copies; no aliasing; no cycles; **no GC**. Objects "mutate" only via `clone with` / `inout`. | **Drops the GC entirely** — milestone is pure mutation. BUT: a Phorge program that aliases-then-mutates an object prints differently from PHP → **the M7 oracle fails** unless objects are *never* aliased-then-mutated, which the checker would have to *prevent* (a uniqueness/linearity discipline — large new checker surface, no PHP analog). Diverges from the single most-ingrained PHP mental model (`$a = $b` on an object shares). |
| **(C) Hybrid: COW objects + immutable-default makes the divergence unobservable** (vm-impact §3 lean) | Objects are `make_mut` COW, but because mutation is opt-in and the checker forbids mutating an instance reachable through two live bindings, the COW-vs-reference difference is never observed. | Middle path: no GC, but needs the same uniqueness-tracking checker as (B), and it is *fragile* — the first hole in the uniqueness check is a silent `run ≢ PHP` divergence the oracle catches only if an example happens to alias. |

**Recommendation: (A) — PHP-faithful shared-mutable objects.** Rationale under the craftsmanship-apex
lens: the apex filter is *craftsmanship*, and the load-bearing craftsmanship property here is **honesty
of the transpile contract** (Invariant #1 — the "honesty enforcer," per the locked philosophy). Option
(B)'s elegance is purchased by either *silently diverging from the oracle* (dishonest) or *building a
linearity checker with no PHP target* (a large, un-transpilable, un-PHP-familiar mechanism — the exact
PL-theory-maximalism the philosophy memory flags as my recurring bias). (A) keeps `Phorge : PHP :: TS :
JS` literally true for objects, costs one well-understood final slice (a cycle-collector that PHP itself
ships), and *additively* still offers `clone with` + `inout` for the value-update style Tracks 1+2 love
— coexistence, not replacement. **The capability "value-semantics objects" is preserved as an opt-in
(`clone with`); the capability "shared-mutable graph" is preserved as the default — both, not either.**
[Inferred — combines the Verified PHP oracle behavior with the philosophy's honesty+coexistence axioms;
the recommendation itself is a design judgment, so Speculative on the margin.]

> Note for the brainstorm: Tracks 1+2 are *not wrong about the language they describe* — Hylo-style
> mutable value semantics is sound and GC-free. They are answering "what if Phorge weren't pinned to
> PHP's object model?" The fork is really: **is byte-identity with PHP's object aliasing a requirement
> or a default we may break here?** That is a developer-values question, not a technical one — exactly a
> STOP-and-ask per the Autonomy Contract.

### Fork 2 — `clone with` and constructor validation **[gates the M-mut.4 immutable-update slice]**

**Crisp statement:** if a constructor validates invariants (`constructor(int age) { requires(age>=0) }`),
does `p with { age = -1 }` re-run the constructor (safe, but `with` becomes fallible → `T?`) or bypass it
(fast, but can forge an instance the ctor would reject)?

| Option | Blast radius |
|---|---|
| **(A) Re-run constructor** | `with` is fallible → returns `T?` or can fault; "no invalid instances" invariant holds. Heavier ergonomics; every `with` site must handle the optional. |
| **(B) Bypass (C# `record` model)** | `with` is total and fast; can produce a ctor-rejected instance. Matches C# precedent and PHP 8.5 `clone with` (which runs property writes + hooks, *not* the ctor). |
| **(C) Bypass + re-validate only declared invariants** | Total when no invariant exists; fallible only for classes with `requires`. Most "correct" but most machinery. |

**Recommendation: (B) bypass**, matching PHP 8.5 `clone with` exactly (the transpile target) and C#.
Rationale: byte-identity with the PHP 8.5 target is the cheapest-honest path; Phorge's *type* system
already prevents the common errors, and invariant-validation is a separate (deferrable) `requires`/
refinement-types feature. Revisit if/when refinement types land. [Inferred — PHP 8.5 `clone with`
behavior is Verified (byte-identity §2.9 ran `mut_test4`); the recommendation is design judgment.]

### Fork 3 — Cycle-collector algorithm + when it runs **[gates only the final GC slice; deferrable]**

**Crisp statement:** given Fork-1=(A), how is the instance-subset cycle collector built, std-only,
byte-identity-safe?

| Option | Blast radius |
|---|---|
| **(A) Synchronous trial-deletion (Bacon-Rajan, what PHP does)** | Maximally PHP-faithful memory model. But std `Rc` exposes no decrement hook → must replace `Rc` with a hand-rolled `Gc<T>` (refcount + color byte + root buffer) across `value.rs` and every `.clone()` hot path → re-benchmark the M2 P5a 634ms baseline. |
| **(B) Periodic mark-sweep over an instance-only arena** | Simpler to reason about; but the tree-walker has no scannable root set (native Rust stack) → needs an explicit value-rooted environment refactor of the interpreter, or it can't trace. Bigger interpreter change. |
| **(C) Per-request bulk-free for `serve`; accept batch-run leaks (reclaimed by the OS at process exit)** | Cheapest. For batch `phg run`/built binaries a leaked cycle is reclaimed at process exit — never a correctness issue. Only the long-running M6 `serve` worker leaks across requests, and HHVM's request-local-heap precedent means dropping the request's root bindings already reclaims the whole acyclic sub-graph; only cycles survive, handled by one trial-deletion pass per request. |

**Recommendation: (C) for the milestone, with (A) as the bounded fallback** if unrestricted long-lived
cyclic structures become a hard requirement outside `serve`. Rationale: do not build a global always-on
collector for a problem the OS already solves for batch programs; scope the only real leak (`serve`) to a
per-request reclaim. This keeps `Rc`/`Drop` and the P5a hot path intact. [Inferred — gc.md §6 + HHVM
precedent (Verified); the per-request reclaim is the narrowest correct intervention.]

### Fork 4 — Default mutability of method parameters and `for..in` loop variables **[minor; local blast radius]**

**Crisp statement:** immutable-by-default is locked for *declarations*; params and loop vars are an edge
PHP makes mutable.

- **Options:** (A) immutable params + loop vars (value-semantics spirit; `mutable` opt-in); (B) PHP-faithful
  mutable. **Recommendation: (A)**, with `for..in`'s var scoped to the loop body (craftsmanship over the
  PHP foreach-var-persists-after-loop quirk). Blast radius: a one-line checker rule + one differential case;
  reversible. [Inferred — features.md §5.3, byte-identity §2.7.]

---

## 4. Recommended mutation semantics + GC approach (the spine decision, assembled)

**Spine = Fork-1 (A) + Forks 2(B) / 3(C) / 4(A).** Concretely:

- **Value/handle split (FORCED, F1+F2):** collections COW via `Rc::make_mut`; instances shared-mutable.
  This is Swift's value-type/reference-type model and PHP's array/object split, encoded identically.
  Evidence: Track 3's executed PHP oracle is the ground truth; `make_mut` is the std-documented COW
  primitive (vm-impact §3, Verified against `doc.rust-lang.org/std/rc`).
- **GC:** `Rc`/`Drop` for everything; a **cycle-collector scoped to the instance subset**, deferred to the
  final slice, observationally invisible (no destructors). For this milestone, lean on per-process /
  per-request reclaim (Fork-3 C); reserve trial-deletion `Gc<T>` (Fork-3 A) only if a hard requirement
  appears. Evidence: gc.md disqualifies tracing arenas (no tree-walker root set + `unsafe` ban) and shows
  `Rc` is the only model keeping weak-ref observability byte-identical across backends.
- **Why not Tracks 1+2's GC-free pure-value model:** it fails the PHP oracle for aliased-then-mutated
  objects unless a no-PHP-analog linearity checker is built. Honesty of the transpile contract (the
  philosophy's spine-as-honesty-enforcer) outranks the elegance of dropping the GC. The GC-free property
  is still *won* for the entire value-type surface — which is most of it.

**The two-tier cost model (features.md §0, the organizing insight):**

| Tier | Features | New `Op`? | GC? |
|---|---|---|---|
| **Tier 1 — local rebinding** | `=`, `+=`/`-=`/`*=`/`/=`/`%=`, `++`/`--`, `??=`, `while`/`do-while`/C-`for`, while-let, `clone with`, get-hooks | **none** (reuse `SetLocal`, jumps, construction lowering) | **none** |
| **Tier 2 — interior mutation** | mutable fields `o.f = e`, element set `xs[i] = e`/`m[k] = e`, `static mutable`, set-hooks | `SetField`, `SetIndex`, `Dup`, maybe `Get/SetStatic` | **the cycle-collector (instances only)** |

Tier 1 delivers ~70% of the surface with zero GC and de-risks Tier 2 by sizing the mutable surface
precisely before the collector is designed.

---

## 5. Dependent-feature slice sequence + the resolved modifier model

### 5.1 Slice sequence (synthesized from features.md §2 + byte-identity §6 + vm-impact §9-F4)

```
M-mut.1  Mutable locals + reassignment              [Tier 1 · no new Op · no GC]
         └ modifier model lands here (§5.2); Stmt::Assign; E-ASSIGN-IMMUTABLE / E-ASSIGN-TYPE;
           smart-cast invalidation on reassign (Kotlin/TS rule — MANDATORY, S2 interaction);
           interpreter assign(); VM resolve_local+SetLocal; transpiler $x = …
           example: examples/guide/mutation.phg  (with a two-binding scalar case)

M-mut.2  Compound assign + ++/-- + ??=               [Tier 1 · pure desugar · no GC]
         └ += -= *= /= %=  (NOT .=, depends on dropped `.`);  ??=;  n++/n-- (statement form only)
           /= %= route through __phorge_div / __phorge_rem (F7)

M-mut.3  Condition loops                             [Tier 1 · jumps only · no GC]
         └ while, do-while, C-for (escape hatch; for..in stays idiomatic); while-let (if-let sugar);
           break/continue generalize from Wave A

M-mut.4  clone-with / copy-update + get-hooks        [Tier 1 · construction lowering · no GC]
         └ p with { field = expr } → fresh instance (Fork-2=B: bypass ctor, PHP 8.5 clone-with target);
           get-hooks = virtual/computed properties (method-on-read lowering)

────────────────  GC BOUNDARY — everything above ships with ZERO GC  ────────────────

M-mut.5  Value-type interior mutation                [Tier 2 · SetIndex + Dup · still NO GC]
         └ xs[i] = e, m[k] = e, list_push — COW via Rc::make_mut; value types provably acyclic,
           so this lands BEFORE the collector. (Refines features.md, which put SetIndex post-GC:
           byte-identity §6 step 2 is right — value-type element-set needs no GC.)

M-mut.6  Shared-mutable instances + the cycle-collector  [Tier 2 · SetField · GC slice]
         └ instance fields become shared-mutable; eq_val cycle-safe (F4, P0 prerequisite);
           the instance-subset collector (Fork-3); optional `===` via Rc::ptr_eq

M-mut.7  static mutable + set-hooks                  [Tier 2 · Get/SetStatic · on the GC]
         └ shared program-lifetime mutable state (the one place a long-lived cycle roots);
           split out per vm-impact F4 to keep earlier slices global-state-free
```

**Refinement over the raw tracks:** features.md folded element-set (`xs[i]=e`) into the post-GC tier;
byte-identity §2.11 + §6 correctly shows **value-type element mutation needs no GC** (a List can't cycle).
So `SetIndex`/COW element mutation (M-mut.5) ships *before* the collector; only `SetField` on instances
(M-mut.6) crosses the GC boundary. This pulls more value forward and shrinks the GC slice's surface.

### 5.2 The modifier model — RESOLVED (confirm the GA-plan four-axis model, with refinements)

The GA plan paused on confirming this; features.md §3 verifies it against Rust/Swift/Kotlin/C# and finds
it is **the Kotlin model almost exactly** + Swift value-default + C# `with`. **Not a genuine fork** —
three reference languages converge and the parity matrix already presumes immutable/readonly-default.

| Axis | Default | Opt-in | Precedent | Grade |
|---|---|---|---|---|
| **Mutability** | immutable | `mutable` | Kotlin `val`/`var`, Swift `let`/`var`, Rust `let`/`mut` (all immutable-default) | Verified (features.md §3.1) |
| **Compile-time const** | — (decl form) | `const NAME = <const-expr>` | Kotlin `const val`, C# `const`, Rust `const` — distinct axis from runtime immutability | Verified |
| **Association** | instance | `static` | universal | Verified |
| **Extensibility** | closed/final | `open` | Kotlin final-by-default + `open` | Verified |

**Resolved refinements (record in the GA plan Decisions Log):**
1. **Drop `final` and `readonly` as value modifiers** — `readonly` is subsumed by immutable-default;
   `final`-for-inheritance becomes the default, `open` is the opt-in. Transpiler *may still emit* PHP
   `readonly` as intent (output detail, not a keyword). [Verified — parity matrix lines 98/261/262/286.]
2. **`mutable` is a BINDING modifier, not a type modifier** — lives on `VarDecl` (+ later a
   `Modifier::Mutable` on field/promoted-param), never baked into the type. Avoids a `mutable T` / `T`
   type-pair explosion across `T?` / `A|B` / `A&B` / `List<T>` / generics. Keeps Invariant #9 (untyped
   AST) and the compiler's `CTy` lattice unchanged — **no new `CTy` variant for mutability.**
   [Verified — Rust/Swift precedent (semantics.md §5); `ast::Modifier` already has the slot.]
3. **`const` vs immutable-local are distinct axes** (keep both, like Kotlin `val`/`const val`): an
   immutable local is runtime-fixed-once; a `const` is compile-time-foldable. [Verified — features.md §3.3.]
4. **`open` semantics gate on `extends` (S6)** — reserve/parse the keyword now, wire enforcement at S6.
   Don't block the modifier model on S6. [Inferred — matrix gates extends on S6.]
5. **Spelling: `mutable`** (locked, GA plan) — legible, PHP-register. [Verified.]

**Verdict: CONFIRM the four-axis model as proposed.** It is not a genuine fork (per the Autonomy
Contract); the only adjacent genuine forks are Fork-4 (param/loop-var default) and the spine Fork-1.

---

## 6. New Ops + parity-risk surface + differential-harness extensions

### 6.1 New Op budget (minimal — vm-impact §2 is the authority)

| New Op | Stack effect | `validate` arm | Need |
|---|---|---|---|
| `SetLocal` | **already exists** | exists | reassignment — **0 new** |
| `SetField(name_idx)` | −2 (pop instance, pop value) | join `GetField`/`CallMethod` name-bound arm | mutable field write (M-mut.6) |
| `SetIndex` | −3 (pop container, index, value) | no-index arm (like `Index`) | element set (M-mut.5); polymorphic List/Map |
| `Dup` | +1 | no-index arm | compound-assign on a field/index target without double-evaluating the receiver |
| `Get/SetStatic(idx)` | +1 / −1 | new static-table bound | `static mutable` only (M-mut.7) — split out |
| loop ops | — | — | **NONE** (jumps suffice, F5) |

**Minimum viable: `SetField` + `SetIndex` + `Dup` = 3 new Ops** for the whole core; `Get/SetStatic`
only if static mutable state lands this milestone. Each Op extends the **three coupled matches**
(`vm::exec_op`, `compiler::stack_effect`, `chunk::BytecodeProgram::validate`) in one commit
(Invariant #5; all three are `_`-wildcard-free). [Verified — chunk.rs Op set + the exhaustive `validate`.]

### 6.2 Parity-risk surface (ranked)

| Risk | Severity | Why |
|---|---|---|
| Aliasing observability (COW vs reference) diverging across backends, and vs PHP | **P0** | `agree`/`agree_err` compare only the two Rust backends — both can alias a List wrongly and still agree. PHP array(COW) vs object(ref) differ, so the *correct* answer differs by type. Only the PHP oracle + a two-binding test catches it. |
| `map_set`/`list_set`/`set_field` kernels re-inlined per backend | **P0** | Re-opens the `Op::Neg` drift class (Invariant #3). Single-source in `value.rs`. |
| `eq_val` unguarded recursion on a cycle | **P0** | Divergent stack overflow across backends (native recursion, not frame-counted) → breaks `agree_err`. Must go cycle-safe before object→object mutation (F4). |
| Nested place-store `a.b[i].c = v` COW-up-the-chain | **P1** | Easy to mutate a clone and drop it; read-modify-write chain with `make_mut` at each level. |
| Cycle-collector timing observable | **P1** | Must never emit/reorder; finalizers stay rejected (F12). |
| `++`/`--` on `T?`/non-numeric; PHP string-`++` | **P2** | Checker gates numeric-only; reject the PHP coercion quirk. |
| `while(true){}` runaway | **P2** | Not recursion → `MAX_CALL_DEPTH` won't catch it. Fork: accept hang (PHP parity) vs iteration budget. Recommendation: match PHP (hang is user error). |

### 6.3 How `tests/differential.rs` must extend (byte-identity §5 + vm-impact §7)

The current `agree` (Ok output) + `agree_err` (FaultKind) oracle is **necessary but not sufficient** for
mutation. Required new cases — all in the PHP-gated `examples/**/*.phg` glob (`PHORGE_REQUIRE_PHP=1`):

1. **Reassignment value:** `var x = 1; x = 2; println(x)` → `2`.
2. **Compound-assign intdiv parity:** `var x = 7; x /= 2; println(x)` → `3`, PHP-oracle-gated (routes
   through `__phorge_div`).
3. **THE aliasing case (P0 catcher), TWO of them — one List, one Map:** `var a = [1,2]; var b = a;
   b[0] = 9; println(a[0]); println(b[0])` → defines COW semantics (`1` then `9`); diverges instantly if
   one backend mutates-in-place while the other clones. (Two cases per the `null-op scratch-slot` lesson.)
4. **Object alias case:** `b = a` on an instance, mutate via one, print via the other → both see it
   (the handle-semantics oracle).
5. **Nested store:** `m["k"][0] = 5` (Map-of-List) — COW-up-the-chain.
6. **Loop mutates iterated collection:** print loop trace + final (F8 non-regression).
7. **Closure capture + mutate-after:** capture an instance, mutate it, call the closure (handle capture
   shares the cell; F-byte-identity §2.6).
8. **`clone`/`clone with`** an instance holding both an object field and a list field, mutate the copy,
   print both (shallow/deep correctness, §2.9).
9. **Fault parity:** `SetIndex` OOB write → identical `FaultKind` both backends (reuse `IndexOob`).
10. **Cycle to completion (GC slice only):** build a cycle, run to completion — must not OOM-crash;
    output collector-independent (run with collection forced-every-alloc vs never).

`phg bench` (Invariant #11): the immutable `GetLocal`/`GetField` read path must stay a refcount bump —
re-run the M2 P5a 634ms object-heavy workload before/after; no regression. A `.borrow()` on every
instance field read (if Fork-1=A is implemented with `RefCell`) is the specific thing to measure.

---

## 7. Open questions still needing a real-php check

These are checkable facts not yet executed against the PHP 8.6 oracle (Track 3 covered the core split,
but these specific behaviors were not in its `mut_test*.php` set):

1. **PHP 8.5 `clone with` + property hooks interaction** — does `clone $o with [...]` *run* a `set`
   hook on the overridden property, or write the backing store directly? (Decides whether get/set-hooks
   compose with `clone with`, and whether Fork-2=B "bypass ctor" also bypasses hooks.) [Unverified — needs
   a `clone with` + `{ set => }` program run under `php -n`.]
2. **`++`/`--` overflow at `PHP_INT_MAX`** — PHP silently promotes `PHP_INT_MAX + 1` to **float**;
   Phorge's `int_add` kernel *faults* on overflow. Confirm the exact PHP output so the checker's
   numeric-only `++` either matches (fault) or the divergence is documented in KNOWN_ISSUES. [Unverified —
   needs `$x = PHP_INT_MAX; $x++; var_dump($x)` under `php -n`.]
3. **Compound `%=` with negative operands** — PHP `%` sign-follows-dividend; confirm Phorge `int_rem`
   already matches across `-7 %= 3` / `7 %= -3` (it should via the shared kernel, but it has not been run
   as a compound-assign through the oracle). [Unverified — needs the four sign combinations executed.]
4. **`static mutable` initializer timing** — PHP runs a `static $n = expr;` initializer once on first
   call; confirm whether `expr` may reference call-time state (it can't — must be const-ish) so the
   program-level slab semantics match. [Unverified — needs a `static $n = f();` probe under `php -n`.]
5. **`foreach` over a Map being mutated mid-loop (k=>v)** — Track 3 verified List foreach-iterates-a-copy;
   the just-adopted Map/Set foreach (parity line 89/423) needs the same snapshot-semantics check under
   mutation. [Unverified — needs a `foreach($m as $k=>$v){ $m[...]=...; }` run.]

---

*STATUS: Designed — research synthesis only, not implemented. The spine (value/handle split → instance-
scoped cycle collector) is FORCED by the executed PHP oracle + Invariant #1, resolving the Track 1/2 vs
Track 3/5 contradiction in favor of PHP-faithfulness. Four genuine forks remain for the developer
(Fork-1 the entire-milestone gate); the four-axis modifier model is confirmed (not a fork). Brainstorm
the forks before design freeze, per the Autonomy Contract's stop-on-genuine-forks clause.*
