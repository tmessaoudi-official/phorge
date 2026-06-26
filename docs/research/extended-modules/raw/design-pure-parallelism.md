# Design — Pure Data-Parallelism (`Core.Parallel`: parallelMap / fork-join)

**Stage 2 design — extended-modules research.**
**Verdict: Tier A (gated, byte-identity-safe by construction). Recommended for adoption.**
**No new VM `Op`. No new `Value` variant. HigherOrder natives only.**

---

## 1. The one-sentence thesis

`Core.Parallel.map(xs, pureFn)` is **`Core.List.map` wearing a permission slip**. It is *semantically*
identical to a sequential map with an input-order-preserving merge; the word "parallel" grants the
*implementation* permission to evaluate the bodies concurrently **later**, but that permission is
unobservable because the result is forced into submission order. Therefore it ships **today running
sequentially** and is **byte-identical to `List.map` on all three legs by construction** — there is no
non-determinism to quarantine, so it is **Tier A and fully gated in `tests/differential.rs`** exactly
like `List.map`/`filter`/`reduce`. (Verified: `list_map` in `src/native/list.rs:189` is a plain
`for x in xs { out.push(call(f, vec![x.clone()])?) }` over a `Value::List`, returning a `Value::List`
in input order — the parallel native is the *same loop* with a stronger doc contract.)

This is the **strongest Tier-A case in the entire concurrency space** (the digest grades it
"essentially FREE today" / "the parallel is an unobservable implementation freedom"), because unlike
the cooperative scheduler / channels / actors, it requires **zero new runtime substrate**: the existing
`NativeEval::HigherOrder` + `ClosureInvoker` machinery (verified in `src/native/mod.rs:65-87`) already
does everything needed.

---

## 2. Scope of this slice

Three primitives, all HigherOrder natives in a new `src/native/parallel.rs` (`Core.Parallel`):

| Native | Signature (Phorge) | Sequential semantics today | PHP transpile |
|---|---|---|---|
| `Parallel.map` | `<T,U>(List<T>, (T) -> U) -> List<U>` | identical to `List.map` | `array_map($f, $xs)` |
| `Parallel.reduce` | `<T,U>(List<T>, U, (U,T) -> U) -> U` | identical to `List.reduce`, **left-fold, fixed order** | `array_reduce($xs, $f, $init)` |
| `Parallel.forkJoin` | `<T>(List<() -> T>) -> List<T>` | run each thunk in list order, collect in list order | `array_map(fn($t) => $t(), $tasks)` |

`forkJoin` is the structured fork-join primitive (the digest's "single best structured primitive to
lift from Go" / rayon `join`): a list of **nullary** task closures, results emitted in **submission
order** regardless of (future) completion order. `map` is the keyed-by-input-position variant.
`reduce` is included only as a *left-fold over the materialised parallel results* — the parallelism is
in producing the elements, not in the fold (the fold itself is inherently sequential; see §8 open
question on associative tree-reduce).

**Deferred to a later slice (named, not silent):** `Parallel.mapReduce` (parallel map + associative
tree-combine — needs a declared-associative combiner, see §8); chunked / batched fan-out tuning knobs;
the *physical* Rust-thread backend (§6). This slice ships the **contract and the sequential
implementation** only.

---

## 3. Phorge-syntax API sketch

```phorge
package Main;
import Core.Console;
import Core.List;
import Core.Parallel;

// A pure, side-effect-free transform.
function heavy(int n) -> int {
    return n * n + 1;
}

function main() -> void {
    var xs = [1, 2, 3, 4, 5];

    // parallelMap — input-order-preserving, == List.map(xs, heavy) byte-for-byte.
    var ys = Parallel.map(xs, heavy);          // [2, 5, 10, 17, 26]
    Console.println("{List.length(ys)} results");

    // Same with a lambda literal (closures must be pure — see §5).
    var zs = Parallel.map(xs, fn(int n) => n * 2);   // [2, 4, 6, 8, 10]

    // fork-join over a list of nullary thunks; results in submission order.
    var tasks = [
        fn() -> int => heavy(10),
        fn() -> int => heavy(20),
        fn() -> int => heavy(30),
    ];
    var results = Parallel.forkJoin(tasks);    // [101, 401, 901]

    // parallel reduce (left-fold over the materialised results).
    var total = Parallel.reduce(ys, 0, fn(int acc, int y) => acc + y);
    Console.println("total = {total}");        // total = 60
}
```

`examples/guide/parallelism.phg` ships in the same change (the standing "examples ship with features"
rule), auto byte-identity-gated by the `examples/**/*.phg` glob in `tests/differential.rs`.

---

## 4. Byte-identity argument (why Tier A, fully gated)

The three-leg spine (`run` ≡ `runvm` ≡ real `php -n` 8.5) holds **by construction**:

1. **`run` ≡ `runvm`.** Both backends dispatch `Op::CallNative(idx, argc)` to the *same* `eval` body
   (the `HigherOrder(parallel_map)` fn pointer). The interpreter supplies `call_closure` as the
   `ClosureInvoker`; the VM supplies `call_closure_value`/`run_until` (verified
   `src/native/mod.rs:67-69`). The body is a `for` loop in input order pushing `call(f, [x])?` into a
   `Vec` — identical control flow to the already-parity-proven `list_map`. A closure fault propagates
   as a plain `String` classified identically by both backends (the established HigherOrder contract,
   and the M-RT S7b-3 fault-parity precedent). **There is no ordering freedom in the shipped code** —
   the loop is sequential, so there is nothing for the two backends to disagree about.

2. **Rust legs ≡ PHP leg.** `Parallel.map` erases to `array_map($f, $xs)` — the *exact same PHP target
   as `List.map`* (verified `src/native/list.rs:292`). `array_map` is order-preserving over a list
   array, so the PHP output is byte-identical to the sequential Rust loop. `forkJoin` erases to
   `array_map(fn($t) => $t(), $tasks)` (call each thunk in order). `reduce` reuses `List.reduce`'s
   `array_reduce` mapping.

3. **`pure: true`.** Because the shipped implementation is deterministic and order-fixed, the natives
   are marked `pure: true`, so `uses_impure_native` (`tests/differential.rs:916`) does **not** skip
   them — they are *fully gated* by the oracle, the same as `List.map`. This is the key difference from
   `Core.Process`: parallelism-as-permission introduces **no** non-determinism, so it does not need the
   quarantine seam at all.

The future physical-parallelism optimisation (§6) **preserves this argument**: the ordered-merge
contract means the observable result (a `Value::List` in submission order) is invariant to evaluation
order. Physical parallelism changes *when* bodies run, never *what order results land in the output
vector*. The byte-identity test continues to pass unchanged.

---

## 5. Rejecting side effects in a parallel task (the central correctness question)

A "pure data-parallelism" primitive is only sound if the bodies are genuinely side-effect-free —
otherwise the *future* physical-parallel backend would expose non-deterministic interleaving (and even
today a body that prints would interleave once threads exist). Two complementary defences, **both
front-end-only (checker), no runtime cost, no backend change:**

### 5.1 Primary: a checker purity rule on the closure argument (`E-PARALLEL-IMPURE`)

When the checker types a call to a `Core.Parallel` native, it walks the **closure argument's body**
(and, transitively, any named function it calls) and rejects it if it reaches an **impure native** —
i.e. any `NativeFn` with `pure == false` (today `Core.Process`/`Core.Env`; the set is read from the
registry, never hardcoded — same single-source discipline as the differential seam). This is the
*exact inverse* of the quarantine flag already in the registry: the differential uses `pure` to decide
what to *skip*; the checker uses it to decide what a parallel body may *call*.

```
E-PARALLEL-IMPURE: a function passed to Core.Parallel.* must be pure
  --> a body (transitively) calling Core.Process / Core.Env / a future impure native is rejected.
```

What "impure" covers, **honestly scoped**:
- **Reliably caught:** calls to `pure: false` natives (the only *ambient-nondeterministic* surface
  Phorge has — clock/env/process). This is the real hazard for the physical backend.
- **Not a hazard in Phorge's model, so deliberately allowed:** `Console.println` (a `pure: true`
  output-buffer append). Output ordering *would* be observable under physical threads, so the physical
  backend (§6) must keep per-task output buffers and concatenate them in submission order — which it
  already must do to preserve byte-identity. So `println` inside a parallel body is **allowed** and
  remains deterministic because the merge is ordered. (Verified the output buffer is a `&mut String`
  threaded through `Pure` natives, `src/native/mod.rs:81`; the HigherOrder invoker reaches it via the
  invoked closure, not the native directly.)
- **Mutation of shared instance state:** Phorge instances are `Rc`-shared *mutable* (M-mut). A parallel
  body that mutates a captured instance is a genuine data race under physical threads. **This slice
  rejects capture of a mutable binding into a parallel closure body** via a second guard
  (`E-PARALLEL-CAPTURE`): a `Core.Parallel` closure may capture only **immutable** locals (the common
  case — captures are by-value and the heap is immutable-by-default; M-mut made *some* paths mutable).
  Lambdas already cannot capture `this` (`E-LAMBDA-THIS`, verified shipped in M3 S3), so method-state
  mutation is already blocked; this guard extends the same conservatism to mutable locals.

### 5.2 Why a checker rule and not a Tier-B escape

The locked decision is *pure data-parallelism = Tier A*. The whole value proposition is that the body
is pure, so we **enforce** purity rather than quarantining impurity. An impure-but-parallel need (fan
out real subprocesses / sockets) is a **different feature** — it is the Tier-B "live escape" that
belongs with `Core.Process` (`proc_open` fan-out), explicitly *out of scope here* and fixture-tested
outside `differential.rs` if/when built. Keeping the two apart is what lets `Core.Parallel` stay fully
gated.

**Conservative-but-sound stance:** the checker walk is a *deny-list of known-impure reaches*, not a
total effect system. A genuinely pure body that the walk can't fully resolve (e.g. an indirectly-passed
first-class fn value whose target the checker can't see) is **rejected with `E-PARALLEL-OPAQUE`** rather
than waved through — fail closed. First-class fn *values* into a parallel native are therefore deferred
(KNOWN_ISSUES), consistent with the existing "cross-package fn values deferred" limitation from M3 S3.

---

## 6. std-only Rust feasibility (today sequential; later physical) given `Rc`-not-`Send`

**Today (this slice): trivially feasible, std-only, zero new deps.** The native body is a sequential
`for` loop over `Value`s on one thread — `Rc` never crosses a thread boundary. Identical to the
shipped `list_map`. No `std::thread`, no `Send`, nothing new.

**Later (the physical optimisation, explicitly deferred): feasible with std `std::thread::scope`, but
gated behind a snapshot/clone-to-owned boundary because `Value` is `!Send`.** This is the one
genuinely hard part and is *why it is deferred, not shipped*:

- `Value` holds `Rc` (`Value::List(Rc<Vec<Value>>)`, instances, etc.) → `!Send`/`!Sync` → cannot be
  moved into a `std::thread`. (Verified premise from the project brief: heap is `Rc`-shared,
  single-threaded forced.)
- The escape that preserves correctness: **convert each task's inputs to an owned, `Send`-able
  representation before spawning, run the *pure* body on a worker, then re-hydrate the result into
  `Value` on the main thread.** Because the body is checker-proven pure (§5), its only inputs are its
  arguments and immutable captures, and its only output is a return value — all of which are
  *data* (ints, floats, strings, owned lists/maps of those). A `Value → OwnedValue (Send) → Value`
  marshalling pass (a deep clone to an `Rc`-free mirror type) makes the worker self-contained. This is
  the **same constraint the digest names** ("needs Send-able task inputs or process fan-out").
  - Marshalling cost may exceed the parallel win for small/cheap bodies → the physical backend is an
    **opt-in / size-heuristic** optimisation, never the default, and **must** be validated by `phg
    bench` showing a real before/after win on a heavy workload (the standing perf-gate rule). If it
    doesn't pay, it doesn't ship — exactly how the M2 P5 slab-arena was rejected for lack of evidence.
  - Even then it must re-merge results into **submission order** (an indexed `Vec<Option<Value>>`
    filled by `(index, result)` pairs), so byte-identity is preserved.
- **Alternative escape (also std-only): process fan-out** (the `proc_open`-shaped path) — but that is
  the impure Tier-B live escape, not this pure primitive, and would break byte-identity (separate
  processes). Rejected for `Core.Parallel`.

**Conclusion:** ship the sequential native now (std-only, free); treat physical threading as a future,
bench-gated, output-preserving optimisation behind the owned-marshalling boundary. Mark this clearly in
KNOWN_ISSUES so nobody assumes today's `Parallel.map` actually uses cores.

---

## 7. New VM `Op` / `Value`? — **No.**

- **No new `Op`.** All three natives are `NativeEval::HigherOrder`, dispatched by the existing
  `Op::CallNative(idx, argc)` (the same op `List.map`/`filter`/`reduce` use). The "adding an `Op` needs
  three coupled matches" tax (`vm.rs exec_op` / `chunk.rs validate` / `compiler.rs stack_effect`) is
  **not incurred**. Verified `List.map` is `HigherOrder` with no dedicated op (`src/native/list.rs:290`).
- **No new `Value`.** Inputs and outputs are existing `Value::List` / `Value::Closure`. `forkJoin`
  takes a `List` of `Closure`s — no new container. (A future `Value::Channel`/`Value::Future` belongs to
  the *cooperative scheduler* slice, a different feature; `Core.Parallel` needs none of it.)
- **Generic-native plumbing already exists.** The signatures use `Ty::Param` (`t()`/`u()`), routed
  through the generic-native call path (`check_native_call` → `check_generic_call`, M-RT S7b-1), the
  same path `List.map` uses. No checker plumbing beyond the two new diagnostics.

Net new surface: one file `src/native/parallel.rs` (three `NativeFn` entries + three small `fn`
bodies that are near-copies of `list_map`/`list_reduce`), three registry registrations, two checker
diagnostics (`E-PARALLEL-IMPURE`, `E-PARALLEL-CAPTURE`) + the fail-closed `E-PARALLEL-OPAQUE`, one
guide example, KNOWN_ISSUES + `phg explain` entries.

---

## 8. Named determinism risks

1. **`reduce` order / associativity.** `Parallel.reduce` is shipped as a strict **left-fold in input
   order** — *not* a tree-reduce — so it is deterministic and equal to `List.reduce`. The genuinely
   parallel, tree-shaped combine (rayon `reduce`) needs a **declared-associative** combiner to be
   order-independent; Phorge has no way to assert associativity, so a tree-reduce would produce a
   *different* (still deterministic, but not left-fold-equal) result and is **deferred** (`Parallel.mapReduce`,
   §2). Risk avoided by not shipping tree-reduce this slice.
2. **Future physical backend output interleaving.** If/when worker threads run bodies that call
   `Console.println`, naive shared-buffer appends would interleave non-deterministically. Mitigation
   (already required by byte-identity): per-task buffers concatenated in submission order. Risk is
   *carried by the deferred optimisation*, not this slice.
3. **Float / `decimal` semantics inside bodies.** No new risk — a parallel body runs the same arith
   kernels (`value.rs`, single-sourced) as a sequential one; the known irrational-float divergence
   (`sqrt(2.0)`) is a pre-existing stdlib limitation, not introduced here. Guide example keeps to
   exactly-representable values.
4. **Closure capture-by-value snapshot timing.** Captures are by-value at closure-creation (M3 S3).
   Sequentially this is unobservable; under physical threads each worker gets the captured snapshot, so
   no shared read after spawn. The `E-PARALLEL-CAPTURE` guard (immutable captures only) closes the only
   remaining shared-mutable hazard.
5. **Fail-open purity walk.** If the checker walk missed an impure reach (a bug), today nothing breaks
   (sequential), but the future physical backend could race. Mitigation: fail-closed on opaque callees
   (`E-PARALLEL-OPAQUE`) + the purity walk is unit-tested against every `pure:false` native.

---

## 9. Effort

**Small–Medium.** The runtime is three near-copies of existing HigherOrder bodies + registry entries
(an afternoon, mechanically). The real work is the **checker purity walk** (`E-PARALLEL-IMPURE` /
`-CAPTURE` / `-OPAQUE`) — a recursive AST visitor over a closure body resolving native calls and
named-fn calls, reusing the registry's `pure` flag and `ast::free_vars`. No backend changes, no new
Op/Value, no transpile-helper. Estimate: **Medium** (one focused session incl. the guide example,
differential cases, `phg explain` entries, and KNOWN_ISSUES). The deferred physical backend is a
**separate Large slice** (owned-marshalling + `thread::scope` + bench gate).

---

## 10. Feasibility

**95%.** The sequential primitive is essentially copy-`List.map`-with-a-name; the only design judgement
is the purity-enforcement strategy, which has a clean precedent (the registry `pure` flag) and a
fail-closed fallback. The 5% uncertainty is entirely in the checker walk's completeness (how
aggressively to reject opaque callees) and the developer's appetite for `Parallel.reduce`'s left-fold-
only semantics vs deferring it.

---

## 11. Open questions for the developer

1. **`reduce` in or out of this slice?** Shipping it as a left-fold makes it *literally* `List.reduce`
   with no parallelism benefit at all (the fold is sequential) — arguably misleading. Option A: ship
   only `map` + `forkJoin` (honest: both genuinely parallelisable), defer all reduce to `mapReduce`
   with a declared-associative combiner. Option B: ship `reduce` as documented-left-fold for API
   symmetry. **Recommend A** (don't ship a primitive whose name implies a benefit it can't deliver —
   the philosophy-of-Phorge "no surprises" rule).
2. **`E-PARALLEL-CAPTURE` strictness.** Reject *all* mutable-local captures (simple, conservative,
   maybe annoying), or only those the body actually mutates (precise, more checker work)? Recommend the
   conservative version this slice; tighten later if it bites.
3. **Should `Console.println` inside a parallel body be allowed?** It's `pure:true` and the ordered
   merge keeps it deterministic — but it *looks* like a side effect. Allow (recommended, it's safe and
   useful for progress logging) or reject for conceptual cleanliness?
4. **API home:** `Core.Parallel` (new leaf, recommended — discoverable, mirrors `Core.List`) vs folding
   `parallelMap` into `Core.List` as `List.parallelMap`? Recommend a dedicated `Core.Parallel` leaf so
   the purity contract has an obvious documentation home.
5. **Commit to the physical backend on the roadmap, or leave it as a perf-bench-gated "if it pays"
   item?** Recommend the latter — ship the contract now, prove the win before building the threading.
