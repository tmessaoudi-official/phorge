# Track 2 — Garbage Collection Strategy (Mutation Milestone)

> Research deliverable for the Phorj mutation + memory-reclamation milestone. Grounded in the repo
> (read, not recalled); external comparisons cited; every claim graded.
> Author pass: 2026-06-21. Companion to Track 1 (mutation semantics) — the two are coupled.

---

## 0. The one-line answer

**A tracing GC is almost certainly NOT required for this milestone, and probably never for Phorj's
core model.** Given Track 1's locked direction — *immutable-by-default + opt-in `mutable`*
[Verified: `docs/plans/2026-06-21-ga-direction-and-autonomy.plan.md` lines 13-28, 87] — the cheapest,
most-craftsmanship-sound, byte-identity-safe, std-only path is **(a) avoid cycles by design**, keeping
pure `Rc`/`Drop` reclamation, and reaching for a cycle collector **only if and when a feature is added
that can actually form a cycle**. The cycle-forming features can be sequenced *last* (or declined), so
GC is a contingent, deferrable sub-decision, not a milestone blocker.

This is exactly Swift's bet (ARC, no cycle collector) and it is the design the current heap already
embodies.

---

## 1. Ground truth from the repo (what actually constrains us)

| Fact | Evidence |
|---|---|
| Every heap value is `Rc<T>` with an **immutable interior** — `List(Rc<Vec<Value>>)`, `Map`, `Set`, `Instance(Rc<Instance>)`, `Enum`, `Closure`, `Bytes`. **No `RefCell` anywhere.** | [Verified: `src/value.rs` lines 12-44; `grep RefCell src/` returns nothing in value.rs] |
| The heap is **immutable + acyclic**: no reassignment, no post-construction field mutation, ctor args fully evaluated before the instance exists (EV-1). So `Rc`/`Drop` reclaims *completely* — "no cycle can leak, so no tracing collector is needed." | [Verified: `src/value.rs` lines 1-6 module doc; CLAUDE.md M2 P5a note] |
| `Stmt` has **no assignment statement** today — only `VarDecl / Return / If / For / Block / Expr`. There is literally no syntax that mutates an existing binding or field. | [Verified: `src/ast.rs` lines 511-546] |
| `Instance.fields` is a plain `HashMap<String, Value>` (owned, not interior-mutable). Mutating a field would require `&mut Instance`, which `Rc` does not give out while shared. | [Verified: `src/value.rs` lines 69-73] |
| Adding any `Op` variant requires extending **three exhaustive matches** in one commit: `vm::exec_op`, `compiler::stack_effect`, `BytecodeProgram::validate`. 44 variants today. | [Verified: INVARIANTS #5; `src/chunk.rs` Op enum] |
| **Byte-identity spine** (`run ≡ runvm ≡ real PHP`) is the central correctness contract; any nondeterminism observable in output breaks it. Determinism rule #8: even `HashMap`-derived lists must be sorted before rendering. | [Verified: INVARIANTS #1, #8] |
| `__destruct` is **rejected** precisely because "destruction timing non-deterministic under `Rc`/`Drop` → breaks the byte-identity spine." `WeakMap` rejected ("needs mutation + weak refs + GC"). | [Verified: parity spec lines 172, 183, 462] |
| The mutation milestone's *payload* (what it unblocks): compound assigns, `++`/`--`, `??=`, `while`/`do-while`/C-for, static/global mutable state, `clone`-with, property set-hooks, while-let, persistent collections. | [Verified: parity spec line 503; plan line 122] |

**Critical observation:** *Mutation does not automatically imply cycles.* A cycle requires (1) a
reference-semantic heap object that (2) can hold a reference to another such object in a field that
(3) can be reassigned after construction to point "backward." Several of the mutation-milestone
features (compound assign on a *local* `int`, `++`/`--` on a counter, `while` with a mutable
loop variable, `??=` on a local) involve **no heap aliasing at all** and **cannot** create a cycle.
Only *field reassignment on a shared object* can. That is the single sub-feature that even raises the
GC question.

---

## 2. The cycle problem, stated precisely

A leak under pure `Rc` needs all three:

1. **Reference semantics for objects** — `a` and `b` are the *same* heap cell when aliased (PHP object
   semantics; `Instance(Rc<Instance>)` already gives this on read).
2. **Post-construction field mutation** — `a.next = b;` reachable as a statement.
3. **A back edge** — `b.next = a;` (or any longer cycle `a→b→c→a`).

Today Phorj has (1) on read but lacks (2) and (3) — there is no assignment statement at all. The
mutation milestone *introduces* (2)/(3) **only if** it allows reassigning a `mutable` field that holds
a reference-semantic object. Local-variable mutation, value-type mutation, and collection mutation that
does not introduce object↔object back-pointers do not create cycles.

The canonical leak (from the prompt): `a.next = b; b.next = a;`. Under pure `Rc`, neither refcount ever
reaches 0 → permanent leak. PHP solves this with a synchronous Bacon-Rajan cycle collector
[Verified: php.net — "PHP implements the synchronous algorithm from *Concurrent Cycle Collection in
Reference Counted Systems* since 5.3.0"]. Swift declines to solve it and pushes the cost onto the
programmer (`weak`/`unowned`) [Verified: Swift ARC is "deterministic and NOT a garbage collector";
"not cycle-collecting … we must write boilerplate to break the cycle"].

---

## 3. The five options, evaluated against Phorj's actual constraints

The non-negotiable filters for every option: **std-only** (no crates), **byte-identical determinism**
(GC must be invisible to output — no observable finalization), **two backends must agree** (the
tree-walker and the stack VM must root-scan compatibly), **craftsmanship-apex** (legible, SOLID, not a
footgun), and **PHP-mappable** (the transpile contract).

### (a) Avoid cycles by design → NO GC  ★ RECOMMENDED (primary)

Keep `Rc`/`Drop`. Forbid the *one* construct that can create a cycle: a `mutable` field (or local)
whose **type can transitively reference its own owner**, i.e. a back edge. Three concrete sub-designs,
in increasing permissiveness:

- **a1 — Value semantics for mutation (copy-on-write).** Mutation produces a *new* value
  (functional update / "clone-with"), never an in-place aliased write. `Rc::make_mut` gives copy-on-
  write for free in std: if the `Rc` is uniquely owned, mutate in place; otherwise clone then mutate.
  No aliasing ⇒ no cycles ⇒ `Drop` is complete. This is the **Clojure/Haskell persistent-structure
  model** [Verified: web — "you can simulate value semantics on top of shared immutable state";
  "aliasing is harmless in the absence of mutation"]. **Cost:** PHP has *reference* object semantics,
  so a Phorj `mutable` object that transpiles to a PHP object would diverge (PHP aliases, Phorj
  copies) — a byte-identity hazard unless Phorj `mutable` objects transpile to PHP *arrays/values* or
  the language declares objects value-typed. This is a **Track 1 semantics fork**, not a GC decision.

- **a2 — Reference semantics but forbid back-references structurally.** Allow `a.field = b` (aliased,
  PHP-faithful) but make the *type system* reject a field assignment that could close a cycle. The
  immutable+acyclic invariant is preserved by construction: a field of type `T` may only be reassigned
  to a value whose type cannot transitively reach `T`'s declaring class. This is an **occurs-check /
  acyclicity check** at compile time. **Cost:** genuinely restrictive (no doubly-linked lists, no
  parent pointers, no graph with back-edges) and the check is non-trivial to specify soundly across
  generics/unions/interfaces. Likely too restrictive for "feature-complete vs PHP."

- **a3 — Reference semantics, back-references via an explicit `weak` reference type.** The Swift model:
  `a.next: Node` (strong) and `b.parent: weak Node` (non-owning). A `weak` field maps to Rust
  `Weak<T>` (std, no crate) [Verified: `std::rc::Weak`; web — "parent uses `Weak`, child uses `Rc`;
  upgrade returns `Option`"]. Cycles are impossible because every cycle must cross at least one `weak`
  edge, and `weak` edges don't keep the target alive. **Cost:** the programmer must annotate back-edges
  (boilerplate, Swift's known ergonomic tax); a `weak` read yields `T?` (it may have been dropped) —
  which *fits Phorj's existing optional model `T?` perfectly* and is a legible, no-surprise contract.
  Transpiles to PHP `\WeakReference` (PHP 7.4+) — a real, idiomatic target.

**Verdict on (a):** This family keeps the entire current architecture (`Rc`/`Drop`, no root-scanning,
no `Op` changes, no VM/interpreter divergence risk, perfectly deterministic — `Drop` order under `Rc`
is already deterministic and, crucially, **not observable** because `__destruct` is rejected). It is
the craftsmanship-apex answer *if* the language can express back-references via `weak` (a3) or live
without them (a1/a2). a3 is the strongest: it is SOLID (explicit ownership), legible, std-only,
PHP-mappable, and reuses Phorj's optional `T?` semantics.

### (b) Rc + cycle collector (Bacon-Rajan / trial deletion) — what PHP itself does

Keep `Rc`, add a synchronous cycle collector that periodically scans "suspect" objects (those whose
refcount was decremented to non-zero — PHP's "purple" roots) using trial deletion: tentatively
decrement, find subgraphs that drop to 0, reclaim them [Verified: Bacon-Rajan 2001; php.net root buffer
of 10,000]. **This is the maximally PHP-faithful option** — Phorj would have the same memory semantics
as the transpile target.

**Cost analysis for Phorj:**
- **Determinism / byte-identity:** PHP runs cycle collection when the root buffer fills (every 10,000
  suspected roots) [Verified: php.net]. Collection *timing* is non-deterministic relative to program
  logic — but it is **invisible to output** *as long as no finalizers run* (Phorj has rejected
  `__destruct`, so nothing observable happens at reclamation). So the byte-identity spine survives
  **iff** Phorj keeps refusing finalizers. This is the load-bearing constraint and it is already met.
- **Std-only:** implementable in std (it is just graph bookkeeping over `Rc` with an internal "color"
  byte per object). But `std::rc::Rc` does **not** expose the refcount-decrement hook or a per-object
  color field — you cannot intercept `Drop` to enqueue purple roots. You would have to **replace `Rc`
  with a hand-rolled `Gc<T>`** that wraps an internal refcount + color + a global root buffer. That is
  a significant, invasive rewrite of `value.rs` and every `.clone()` hot path (the `Op::GetLocal`
  refcount-bump path that M2 P5a tuned to 634 ms). [Inferred: from `std::rc::Rc` API surface — it has
  no decrement callback; confirmed by the Swift/HHVM designs all using custom heap objects, not a
  library smart pointer.]
- **Two backends:** the collector operates on the shared `Value` heap, so both backends get it for
  free — *good*, no root-scanning divergence (the roots are the same `Value`s in locals/operand stack).
- **Craftsmanship:** correct, battle-tested algorithm, but heavy machinery for a language whose default
  is immutable. Pays a permanent per-object space + bookkeeping cost for a problem only a *minority* of
  `mutable` programs even have.

**Verdict:** The "correct PHP-twin" answer, but **disproportionate** to the need under an
immutable-default language, and it forces abandoning std `Rc` for a custom `Gc<T>`. Reserve as the
fallback *if* unrestricted mutable back-references become a hard requirement.

### (c) Mark-sweep tracing GC over an arena (Wren / Crafting Interpreters / early Lua)

Replace `Rc` entirely with an arena of GC objects; periodically trace from roots (locals, operand
stack, globals), mark reachable, sweep the rest [Verified: Nystrom's bi-color mark-sweep, "the actual
algorithm used in early Lua"; heuristic = "is live memory 2× the last GC?"].

**Cost for Phorj:**
- **Root-scanning is where the two backends diverge and bite.** The VM has an explicit operand stack +
  reified `Frame { func, ip, slot_base }` stack — its roots are enumerable [Verified: ARCHITECTURE.md
  "vm::Frame is a reified call record on an explicit frame stack"]. The **tree-walker keeps its call
  records on the native Rust stack** (`interpreter::CallScopes` is a block-scope chain) [Verified:
  ARCHITECTURE.md "the tree-walker keeps its call records on the native Rust stack"] — so it has **no
  enumerable root set**; you cannot trace native Rust stack frames in std without conservative stack
  scanning (`unsafe`, forbidden by `#![forbid(unsafe_code)]`, INVARIANTS #10). You would have to
  re-architect the interpreter to hold an explicit value-rooted environment. **This is the killer.**
- **Determinism:** same as (b) — invisible iff no finalizers; survivable.
- **Std-only:** doable (arena = `Vec<Option<Object>>` + freelist + mark bits) but it is a *second*
  memory model living beside nothing — you throw away `Rc`'s automatic, prompt, deterministic
  reclamation and replace it with deferred sweeps and a tuning heuristic.
- **Forbidden-unsafe tension:** real arena collectors usually want raw pointers / `unsafe` for the
  object graph. A safe-Rust arena (indices into a `Vec`) is possible (the `id-arena` pattern) but adds
  an indirection on *every* field access — directly regressing the M2 P5a hot path.

**Verdict:** **Strongly dis-recommended.** Highest complexity, throws away the deterministic-reclaim
property the project deliberately built, and the tree-walker has no root set to scan without an `unsafe`
conservative scanner that the quality gate forbids. The two-backend root-scan asymmetry alone disqualifies it.

### (d) Region / generational

A refinement *of* (c) (generational = young/old spaces; regional = lifetime-scoped arenas)
[Verified: Lua's KGC_GENMINOR/GENMAJOR; HHVM's request-local zoned heap]. HHVM's *request-local* arena
is the interesting one for a transpiler-adjacent language: the whole heap dies at request end, so most
garbage never needs collecting [Verified: HHVM "RDS + 2 stacks are the root set; zoned strategies
partition the heap"].

**Cost for Phorj:** inherits *all* of (c)'s problems (no tree-walker root set, throws away `Rc`,
`unsafe` pressure) and adds write-barrier complexity (generational GC needs a barrier when an old object
points to a young one [Verified: Lua "when a black object references a white object, mark it gray"]).
A write barrier is plausible only *with* mutation, and it is more machinery than the immutable-default
program population justifies. The one genuinely attractive idea — a **per-program/per-request region
that bulk-frees at exit** — is something Phorj effectively already gets: a short-lived `phg run`
process drops its whole `Rc` graph at exit; leaked cycles would be reclaimed by the OS at process end
anyway for batch programs. (That does **not** save a long-running `phg serve` worker — see §6.)

**Verdict:** Dis-recommended as a collector; the "region that frees at exit" insight is real but is an
OS-process property Phorj already has for batch runs, not a reason to build generational GC.

### (e) Rc<RefCell> + Weak by convention

Make objects mutable via `Rc<RefCell<Instance>>` and ask programmers to use `Weak` for back-edges *by
convention* (no enforcement).

**Cost for Phorj:**
- `RefCell` moves borrow-checking to **runtime** — a double-borrow is a `panic!`, which **violates
  EV-7 / INVARIANTS #6** ("never SIGABRT/panic; exit 1 with a clean Diagnostic"). You'd have to catch
  every `borrow_mut` and convert a `BorrowMutError` into a fault — pervasive, fragile.
- "By convention" weak refs means cycles **do** leak when the programmer forgets — exactly the footgun
  the philosophy says to remove ("removes surprises"). It is the *least* craftsmanship-sound option.
- It still doesn't collect cycles; it just makes them *possible* and *unenforced*.

**Verdict:** **Rejected.** Runtime-panic borrow model conflicts with the no-crash invariant, and
"convention" is the antithesis of the no-surprise philosophy. If reference-mutation is wanted, (a3)'s
*typed* `weak` (enforced, yields `T?`) is strictly better than (e)'s convention.

---

## 4. What comparable transpiled / embedded languages do (and what it teaches)

| Language | Model | Cycles handled by | Lesson for Phorj |
|---|---|---|---|
| **Swift** | ARC (deterministic refcount), **no collector** | programmer: `weak`/`unowned` | [Verified] A production language ships *zero* cycle collection and pushes back-edges to a typed `weak`. Validates option (a3). Swift `weak` ⇒ optional — mirrors Phorj `T?`. |
| **PHP** (transpile target) | refcount + **synchronous Bacon-Rajan cycle collector** | runtime, invisible (no user finalizer ordering guarantees) | [Verified] The twin of option (b). Confirms cycle collection can be *output-invisible* — but only because PHP's `__destruct` order is unspecified; Phorj dodges this by rejecting `__destruct` entirely. |
| **HHVM** (the *other* PHP) | refcount + mark-sweep **backup** collector, request-local arena | runtime, deferred to low-pressure periods | [Verified] Even Facebook's PHP keeps refcounting primary and treats tracing as a *backup for cycles only*. Endorses "refcount-first, trace only cycles" (option b layered on a). |
| **Lua / LuaJIT** | incremental tri-color mark-sweep (+ generational) | tracing collector with write barriers | [Verified] Full tracing; needs enumerable roots + write barriers. Phorj's tree-walker lacks the root set — option (c)'s blocker. |
| **Wren / clox** | bi-color mark-sweep, `2×`-growth heuristic | tracing collector | [Verified] Clean & simple *when the VM owns all roots*. Phorj's VM does; its interpreter does not. |
| **mruby** | mark-sweep (incremental, generational since 1.x) | tracing collector | [Inferred: mruby is a tracing-GC embedded Ruby — confirms the embedded-VM norm is tracing, but all such VMs control their root set, unlike Phorj's tree-walker.] |

**Synthesis:** Languages that *own their entire runtime stack* (Lua, Wren, mruby, HHVM) can afford
tracing. **Swift — the closest analog to Phorj's bet (refcounted, deterministic, no observable
finalization)** — deliberately declines tracing and uses typed `weak`. Phorj's dual-backend design,
where the *interpreter has no scannable root set*, pushes it toward Swift's answer, not Lua's.

---

## 5. Determinism & invisibility — the spine, examined directly

The byte-identity spine forbids any GC effect from reaching output. Three sub-claims:

1. **`Rc`/`Drop` (options a, b) is already deterministic *and* invisible.** `Drop` runs at a
   deterministic point (last owner dropped), but Phorj renders nothing on drop (`__destruct` rejected)
   [Verified: parity spec 172, 462]. So even prompt deterministic destruction is output-invisible.
   ✔ spine safe.
2. **A cycle *collector* (b) runs at a non-deterministic time** (buffer-fill / heuristic). This is
   output-invisible **iff no finalizer fires** — which holds as long as Phorj keeps rejecting
   `__destruct`/`__clone`-side-effects. ✔ spine safe *under the existing rejection*. ⚠ If Phorj ever
   adds observable finalization, *every* GC option except (a1 value-semantics) breaks the spine.
3. **Tracing (c, d) sweep order is non-deterministic**, but again invisible without finalizers. The
   real spine risk in (c/d) is not order — it is the **two-backend root-scan asymmetry** (§3c): if the
   VM and interpreter root-scan differently, they could reclaim at different points and, with *any*
   weak-reference observation (`weak.upgrade()` → present/absent), produce **divergent output**. That
   is a latent `run ≢ runvm` bug. Option (a3)'s `weak` must therefore be specified so that *when* a
   weak target is collected is identical on both backends — easiest if collection is **deterministic
   `Rc`/`Drop`** (a) rather than a deferred sweep (c/d). **This is a decisive argument for (a) over
   (c/d): only prompt `Rc` reclamation makes `weak.upgrade()` byte-identical across backends.**

> **Load-bearing finding:** weak-reference *observability* (`upgrade()` → `T?`) is the subtle place
> where a GC choice can leak into output. Prompt `Rc` drop makes it deterministic and identical on both
> backends; any deferred collector makes the *timing* of "the weak became null" backend-dependent.
> ⇒ keep `Rc`. [Inferred: combines INVARIANTS #1 with `std::rc::Weak::upgrade` semantics.]

---

## 6. The one case that can still force a collector: `phg serve`

For **batch** programs (`phg run`/`runvm`/built binary), a leaked cycle is reclaimed by the OS at
process exit — a leak is a bounded annoyance, never a correctness issue, and never observable. So GC is
*genuinely optional* for the batch surface.

The exception is the **long-running M6 `phg serve` worker** [Verified: ARCHITECTURE.md `serve.rs`,
`Transport` seam]: a request handler that builds a cycle leaks that memory *for the life of the
process*, across requests — an unbounded leak. **But:** M6 is single-threaded with a *request-scoped*
value graph, and the HHVM precedent [Verified] is exactly a **request-local heap that bulk-frees at
request end**. Phorj can adopt the same: scope each request's allocations to a region dropped wholesale
when `handle()` returns. Because the heap is `Rc` and a request's graph is rooted only in that request's
locals, **dropping the request's root bindings already drops the whole acyclic sub-graph**; the only
survivors are cycles — and a request-end region-free (or a single trial-deletion pass over that
request's suspect set) reclaims them without a global, always-on collector. This is the **narrowest
possible** place a collector could be justified, and even there a per-request bulk-free is simpler than
a tracing GC. [Inferred: from HHVM request-local model + Phorj's `Rc` rooting.]

---

## 7. Recommendation

**Primary (this milestone): option (a3) — keep `Rc`/`Drop`, add a typed `weak` reference for
back-edges, NO collector.**

- Sequence the mutation milestone so the **cycle-incapable** features land first and need *zero* GC
  work: local reassignment, compound assigns (`+=` etc.), `++`/`--`, `??=`, `while`/`do-while`/C-for
  (mutable loop var), `clone`-with as a *functional* update (`Rc::make_mut` copy-on-write). None of
  these alias object fields into a back-edge. [Verified: feature list, parity spec 503.]
- For the cycle-*capable* feature (reassignable object-typed fields, e.g. linked structures), require
  back-edges to be declared `weak` (→ Rust `std::rc::Weak`, → PHP `\WeakReference`), yielding `T?` on
  read — reusing Phorj's optional model. A non-`weak` field reassignment that the checker proves could
  close a cycle is an error with a clear diagnostic + did-you-mean-`weak` hint. This makes cycles
  **structurally impossible** while still allowing graphs — Swift's proven bet, in a typed,
  no-surprise form.
- **No `Op` changes** for memory management; no `value.rs` `Rc`→`Gc` rewrite; both backends keep
  identical reclamation; the spine is preserved because reclamation stays prompt and invisible.

**Fallback (only if unrestricted mutable back-references without `weak` become a hard GA requirement):**
option (b) — a synchronous trial-deletion cycle collector, accepted *only* with the standing rule that
`__destruct`/observable finalization stays rejected, and built as a custom `Gc<T>` replacing `Rc` (the
expensive part). Even then, prefer scoping it **per-request in `serve`** (§6) over an always-on global
collector.

**Reject:** (c) tracing arena and (d) generational (two-backend root-scan asymmetry + `unsafe` pressure
+ throws away deterministic `Rc` reclaim), and (e) `Rc<RefCell>`+convention (runtime panic vs EV-7;
unenforced footgun vs the no-surprise philosophy).

**Is GC required?** **No — not for this milestone, and likely never for the batch surface.** It becomes
a *narrow, contingent* question only for (i) unrestricted mutable object back-references (avoided by
`weak`) and (ii) the long-running `serve` worker (addressed by a request-local bulk-free). Track 1's
immutable-default + the `weak` escape hatch keep Phorj in the Swift quadrant: deterministic
refcounting, no tracing GC, cycles made impossible by type rather than collected at runtime.

---

## 8. Sources

- Bacon, Rajan — *Concurrent Cycle Collection in Reference Counted Systems* (2001): https://pages.cs.wisc.edu/~cymen/misc/interests/Bacon01Concurrent.pdf ; https://link.springer.com/chapter/10.1007/3-540-45337-7_12
- PHP Manual — *Collecting Cycles* (synchronous Bacon-Rajan since 5.3, 10,000-root buffer): https://www.php.net/manual/en/features.gc.collecting-cycles.php
- HHVM — *On Garbage Collection* (refcount + mark-sweep backup, request-local): https://hhvm.com/blog/431/on-garbage-collection ; memory-management hackers' guide: https://github.com/facebook/hhvm/blob/master/hphp/doc/hackers-guide/memory-management.md
- Swift ARC (deterministic, no collector; weak/unowned): https://www.vadimbulavin.com/swift-memory-management-arc-strong-weak-and-unowned/
- Lua GC (incremental tri-color mark-sweep + generational + write barriers): https://deepwiki.com/lua/lua/5.1-garbage-collection ; https://www.lua.org/wshop18/Ierusalimschy.pdf
- Bob Nystrom — *Baby's First Garbage Collector* / Crafting Interpreters (bi-color mark-sweep, early-Lua algorithm, Wren): https://journal.stuffwithstuff.com/2013/12/08/babys-first-garbage-collector/ ; https://craftinginterpreters.com/garbage-collection.html
- Rust `std::rc::Weak` (back-references, `upgrade() -> Option<Rc<T>>`): https://doc.rust-lang.org/std/rc/struct.Weak.html ; https://doc.rust-lang.org/book/ch15-06-reference-cycles.html
- Persistent data structures / copy-on-write value semantics (aliasing harmless without mutation): https://en.wikipedia.org/wiki/Persistent_data_structure ; https://arxiv.org/pdf/2106.12678

### Repo evidence (read this session)
- `src/value.rs` (Rc-shared immutable Value, no RefCell), `src/ast.rs` (no assignment Stmt),
  `src/chunk.rs` (Op set + 3 coupled matches), `docs/INVARIANTS.md` (#1 spine, #5 Op, #6 EV-7/no-panic,
  #8 determinism, #10 forbid-unsafe), `docs/ARCHITECTURE.md` (VM frame stack vs interpreter native-stack
  scopes; serve.rs), `docs/specs/2026-06-21-php-parity-and-beyond.md` (mutation defers, __destruct/WeakMap
  rejects), `docs/plans/2026-06-21-ga-direction-and-autonomy.plan.md` (immutable-default + `mutable`).
