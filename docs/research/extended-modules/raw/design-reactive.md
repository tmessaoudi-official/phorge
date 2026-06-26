# Design — Reactive / FRP Streams (`Core.Stream`)

**Stage 2 design** · extended-modules research · 2026-06-27
Author: research agent · Status: design proposal (not implemented)

## 0. TL;DR / Verdict

| Subset | Tier | New Op? | Feasibility |
|---|---|---|---|
| **A1** — eager finite `Core.Stream` (Rx-shaped pull pipeline over a List/range source: `of`/`from`/`range`/`map`/`filter`/`scan`/`take`/`drop`/`distinct`/`flatMap`/`merge`/`zip`/`concat` + terminals `collect`/`forEach`/`fold`/`count`) | **A (gated)** | **NO** | **high, ~90%** |
| **A2** — lazy pull `Stream<T>` (cold observable, single synchronous consumer, generator-backed) | **A (gated)** but **deferred to the cooperative-scheduler slice** | likely NO | medium, ~55% |
| **B** — live/hot sources (socket/timer/clock `interval`/`debounce`/`fromSocket`/hot subjects) | **B (quarantined)** | NO (rides serve.rs `Transport` + `pure:false`) | medium |
| **R** — real-thread schedulers, multicast hot subjects with concurrency, backpressure as a runtime protocol | **reject** | — | — |

**Recommendation: ship A1 now as a pure-`HigherOrder`-native library; it is essentially free.**
A1 is *byte-identical by construction* because every operator is a deterministic, order-preserving
transformation of a finite sequence — i.e. it is `List.map`/`filter`/`reduce` wearing a fluent,
composable API. There is **no scheduler, no time, no concurrency** in A1; "reactive" here means the
*operator algebra*, not asynchrony. A2 (true lazy/cold streams) and B (live sources) layer on later
and depend on the cooperative async scheduler (a separate research stage) and the M6 `Transport`
seam respectively.

This matches the prior-art digest's strongest signals: *"ReactiveX operator algebra over an
eager/finite list (immediate scheduler) → Tier-A; already half-shipped as Core.List.map/filter/reduce.
Ship as HigherOrder natives, no new Op"* and *"Lazy pull-based Stream<T> … byte-identical by
construction"* and *"Backpressure … a non-problem … reduces to ordinary lazy evaluation."*

---

## 1. The byte-identity argument (why A1 is Tier A)

Phorge's correctness spine is the three-leg byte-identity in `tests/differential.rs`: interpreter
`run` ≡ VM `runvm` ≡ transpiled PHP under `php -n` 8.5. A feature is Tier A iff its output is a pure
function of the program text, computed in a fixed total order on all three legs.

A1 satisfies this trivially:

1. **Source is finite + ordered + deterministic.** A `Core.Stream` is constructed only from a
   `List<T>` (`Stream.from(xs)`), a varargs/literal (`Stream.of(a, b, c)`), or an integer range
   (`Stream.range(a, b)` ≡ the already-shipped `a..b`). No clock, no random, no socket, no
   environment read. The element sequence and its order are fixed by the source value.

2. **Every operator is a deterministic, order-preserving map over that sequence.** `map`/`filter`/
   `scan`/`take`/`drop`/`distinct`/`zip`/`concat`/`merge`/`flatMap` each have a single defined
   traversal order (left-to-right over the input element vector). This is *exactly* the
   `List.map`/`filter`/`reduce` discipline already shipped and byte-identity-gated (M-RT S7b-3,
   `[higher-order-natives-reentrant-vm]`): the closure runs on the calling backend via the shared
   `ClosureInvoker`, so the interpreter's `call_closure` and the VM's re-entrant
   `call_closure_value`/`run_until` drive the *same* `exec_op`, producing identical results and
   identical faults. A closure fault (or wrong-type return) is captured as a plain `String` and
   classified identically by `agree_err`'s `FaultKind` (`[error-parity-faultkind]`).

3. **The terminal op produces a concrete `Value`** (`collect → List<T>`, `fold → U`,
   `count → int`, `forEach → Unit` with output-buffer side effects in source order). No lazy state
   escapes; the whole pipeline is evaluated eagerly at the terminal, in one synchronous pass.

4. **There is no `merge` interleaving ambiguity.** In Rx, `merge` over *live* sources interleaves by
   arrival time (non-deterministic). In A1 there are no live sources — `Stream.merge(a, b)` over two
   finite streams is defined as **concatenation in argument order** (all of `a`, then all of `b`), or,
   if a "round-robin" merge is wanted, a *fixed* index-interleave (a[0], b[0], a[1], b[1], …). Either
   is a total, source-only order. We pick **concat-in-arg-order as the `merge` semantics** (simplest,
   matches `Promise.all`-style ordered-merge from the digest) and expose round-robin separately as
   `Stream.interleave` if ever needed. This is the *one* place Rx's non-determinism could leak — and
   we close it by definition, exactly as the Go-`select` digest entry resolves "pick FIRST IN SOURCE
   ORDER (not random)."

Because A1 reduces to the same primitive that `List.map`/`filter`/`reduce` already gate
byte-identically, **the byte-identity proof is by reduction**: A1's runtime kernel *is* a sequence of
those list ops, and its PHP erasure *is* a sequence of `array_map`/`array_values(array_filter(…))`/
`array_reduce`/`array_slice`/`array_merge` — all PHP core builtins present under `php -n` (no
mbstring, no Composer, no extension). The differential's existing `examples/**/*.phg` glob auto-gates
the shipped `examples/guide/streams.phg`.

---

## 2. Phorge-syntax API sketch (A1)

`Core.Stream` is a thin **fluent façade** whose values are ordinary `List<T>` carried inside a
one-field wrapper class `Stream<T>` (so the dot-chain reads fluently), OR — the recommended,
lower-risk option — **plain free natives in `Core.Stream` that thread a `List<T>`** and chain via
UFCS (`x.f(a) ≡ f(x, a)`, already shipped, `[ufcs-and-interpolation-span-fix]`). UFCS gives the
fluent feel with **zero new class machinery and zero new Value** — a `Stream` *is* a `List` at
runtime. This is the design I recommend; the wrapper-class variant is in §6 (Open Questions).

```phorge
package Main;

import Core.Console;
import Core.Stream;

function main() -> int {
    // source: a finite, ordered, deterministic List/range
    var total = Stream.range(1, 11)            // 1..11  → Stream<int> (a List<int>)
        .filter(fn(int n) => n % 2 == 0)        // 2,4,6,8,10
        .map(fn(int n) => n * n)                // 4,16,36,64,100
        .take(3)                                // 4,16,36
        .fold(0, fn(int acc, int n) => acc + n); // 56

    Console.println("sum = {total}");           // sum = 56

    // zip + collect
    var names = Stream.of("a", "b", "c");
    var nums  = Stream.range(1, 4);             // 1,2,3
    var pairs = names.zip(nums, fn(string s, int n) => "{s}{n}")
        .collect();                              // List<string> ["a1","b2","c3"]
    Console.println(Stream.of_list(pairs).fold("", fn(string a, string p) => "{a} {p}"));

    // scan = running fold (emits each intermediate accumulator)
    var running = Stream.of(1, 2, 3, 4)
        .scan(0, fn(int acc, int n) => acc + n)  // 1,3,6,10
        .collect();
    Console.println("running = {running}");
    0
}
```

### Operator set (A1)

Constructors (free fns in `Core.Stream`, each returns a `Stream<T>` = `List<T>`):
- `Stream.of(...) -> Stream<T>` — varargs literal (or, until varargs land, `Stream.of_list(xs)`).
- `Stream.from(List<T>) -> Stream<T>` / alias `Stream.of_list`.
- `Stream.range(int, int) -> Stream<int>` — `[a, b)`, ≡ `a..b`.
- `Stream.empty() -> Stream<T>`.

Stateless transforms (HigherOrder, generic `<T>`/`<U>`):
- `.map((T)->U) -> Stream<U>`
- `.filter((T)->bool) -> Stream<T>`
- `.flatMap((T)->Stream<U>) -> Stream<U>` (concat each result in order)
- `.take(int)` / `.drop(int)` / `.takeWhile((T)->bool)` / `.dropWhile((T)->bool)`
- `.distinct() -> Stream<T>` (first-occurrence order; structural `eq_val`)
- `.zip(Stream<U>, (T,U)->R) -> Stream<R>` (truncates to the shorter)
- `.concat(Stream<T>) -> Stream<T>` ; `.merge(Stream<T>)` ≡ `.concat` (arg-order, §1.4)

Stateful/terminal:
- `.scan(U, (U,T)->U) -> Stream<U>` (running fold; emits each accumulator)
- `.fold(U, (U,T)->U) -> U` (≡ `List.reduce`)
- `.collect() -> List<T>` (≡ identity — the wrapper unwrap)
- `.forEach((T)->Unit) -> Unit` (side effects in source order)
- `.count() -> int`

All higher-order ops infer their `<T>/<U>` at the call site through the **existing generic-native
unifier** (`check_generic_call`, M-RT S7b-1) — the registry `Ty::Param` never reaches a backend.

---

## 3. Exact PHP transpile target

Because `Stream<T>` *is* a `List<T>` (= PHP sequential array) and each operator is a list transform,
each native erases to a **PHP core builtin** (all present under `php -n`):

| Phorge op | PHP erasure (core only) |
|---|---|
| `Stream.from(xs)` / `of_list` | `{xs}` (identity) |
| `Stream.range(a,b)` | `range({a}, {b}-1)` (or reuse the existing `__phorge_range` helper that already backs `a..b`) |
| `.map(f)` | `array_map({f}, {xs})` |
| `.filter(p)` | `array_values(array_filter({xs}, {p}))` |
| `.take(n)` | `array_slice({xs}, 0, {n})` |
| `.drop(n)` | `array_slice({xs}, {n})` |
| `.concat`/`.merge(ys)` | `array_merge({xs}, {ys})` |
| `.distinct()` | `array_values(array_unique({xs}, SORT_REGULAR))` |
| `.fold(init,f)` | `array_reduce({xs}, {f}, {init})` (note: reduce-init is Phorge's 2nd arg; arg-order trap per `[higher-order-natives-reentrant-vm]`) |
| `.count()` | `count({xs})` |
| `.zip` / `.scan` / `.flatMap` / `takeWhile` / `dropWhile` | small **gated runtime helpers** (`__phorge_zip`, `__phorge_scan`, …) emitted once when used — the established pattern for ops without a clean single-builtin target (`[php-leg-outside-correctness-loop]`: prefer a helper over fragile static-type-dependent emission) |

No `mb_*`, no extension, no Composer. The helpers are pure PHP loops over arrays — trivially
byte-identical to the Rust kernels. **Recommendation:** implement the non-builtin ops (`zip`, `scan`,
`flatMap`, `takeWhile`, `dropWhile`, `distinct`) as gated `__phorge_*` helpers rather than chasing a
clever builtin composition — the helper is the documented safe fallback and removes byte-identity risk.

---

## 4. New VM Op / Value? — **NO**

- **No new `Op`.** Every operator is an `Op::CallNative(idx, argc)` exactly like
  `List.map`/`filter`/`reduce`. Constructors (`Stream.range`) reuse the already-shipped `Op::MakeRange`
  via desugaring `Stream.range(a,b)` to a native that internally materializes the range (or simply
  call the range native). No coupled-three-match change (`[op-variant-match-coupling]`) is triggered.
- **No new `Value`.** A `Stream<T>` is represented as `Value::List` at runtime (the recommended
  UFCS-over-List design). The fluent type `Stream<T>` exists **only in the checker** as a
  newtype-ish alias that resolves to `List<T>` for operand purposes, OR is a real single-field class
  erased to `array` — either way the runtime carries a plain list, so `CTy` resolution is unaffected
  (`[cty-tracks-operand-types]`: no out-of-surface arithmetic-operand gap, because a `Stream` is never
  an arithmetic operand — `.fold(...)` returns the element/accumulator type which is already a
  first-class operand).
- **Higher-order plumbing already exists.** The `NativeEval::HigherOrder` + `ClosureInvoker` machinery
  (the VM's re-entrant `run_until`/`call_closure_value`) was built for `List.map` and is generic over
  any closure-taking native — adding `Stream.*` ops is purely additive registry entries in a new
  `src/native/stream.rs` leaf, mirroring `list.rs`.

---

## 5. Determinism risks (named)

1. **`merge` interleaving.** *Mitigated by definition* (§1.4): `merge` = concat-in-arg-order; any
   round-robin variant uses a fixed index order. No arrival-time semantics in A1.
2. **`distinct` ordering.** Must be **first-occurrence order** (not sorted, not hash order). Rust
   `Vec` + a seen-`eq_val` scan gives this; PHP `array_unique(…, SORT_REGULAR)` + `array_values`
   preserves first-occurrence order — verify the SORT flag (default `SORT_STRING` would juggle); pin
   `SORT_REGULAR` and add a mixed-type `distinct` case to the example. *Risk: medium — the one place
   PHP's array semantics could diverge; gate with an explicit example.*
3. **Float formatting in `scan`/`fold` results.** Inherits the existing irrational-float divergence
   (`sqrt(2.0)`-class) — examples keep to exactly-representable values (KNOWN_ISSUES), the run↔runvm
   spine is always identical regardless.
4. **`flatMap` over an empty inner stream** — empty `array_merge` returns `[]`; trivial, just test it.
5. **A2/B only:** lazy evaluation order, scheduler ready-queue order, live-source arrival — **none
   present in A1**. These are the reasons A2 waits on the cooperative-scheduler slice and B is
   quarantined.

No clock, no random, no network, no env, no threads in A1 ⇒ no Tier-B determinism risk.

---

## 6. A2 — lazy pull `Stream<T>` (deferred, Tier A when it lands)

A true *cold* observable (lazy, pulled element-by-element by a terminal) is still Tier A — *"byte
identical by construction"* — because a single synchronous consumer with an immediate scheduler has
**one** evaluation order. But it needs either (a) generator-as-coroutine support (`yield`, a
front-end lazy state machine advanced by `.next()` driven by the existing `run_until`), or (b) a
hand-rolled lazy thunk list. Both are **more machinery than A1's eager lists** and overlap heavily
with the cooperative-scheduler design (a separate research stage). **Recommendation: defer A2 until
the scheduler slice; ship A1 first.** A2's payoff over A1 is only infinite/expensive sources
(`Stream.range(0, BIG).take(5)` avoiding full materialization) — a performance refinement, not new
expressiveness. When A2 lands, the *same* operator surface (§2) is reused; only the engine changes
from eager-List to lazy-thunk, and the byte-identity contract is unchanged (immediate scheduler =
total order). PHP leg for A2 = real PHP `Generator`s (`yield` is core, present under `php -n`) or the
same eager array fallback.

---

## 7. B — live / hot sources (quarantined, Tier B)

`Stream.interval(ms)`, `.debounce(ms)`, `Stream.fromSocket(...)`, hot multicast subjects — these read
a real clock / socket / external event, so they are **non-deterministic** and ride the existing
quarantine seam exactly like `Core.Process`: `pure: false` natives, auto-dropped from
`differential.rs` by `uses_impure_native` (derived from the `pure` flag, no harness edit), fixture-
tested in a dedicated `tests/stream_live.rs` with a replay log, transpiled to PHP but **not**
byte-identity-gated. The socket/timer source itself sits behind the M6 `src/serve.rs` `Transport`
trait (the locked quarantine pattern). **Out of scope for the A1 slice; design only.**

---

## 8. Effort

- **A1 (recommended slice):** **small–medium.** A new `src/native/stream.rs` leaf (~mirrors
  `list.rs`'s HigherOrder entries), ~12 registry entries, ~5 `__phorge_*` PHP helpers, a `Stream<T>`
  checker alias-or-wrapper, one `examples/guide/streams.phg`, README + coverage-matrix entry. No
  backend/Op/Value change. Reuses generic-native unifier, `ClosureInvoker`, UFCS. Realistically a
  single focused session.
- **A2 (lazy):** medium — gated on the scheduler slice; design-reuse of A1's surface.
- **B (live):** medium — rides serve.rs/Transport + quarantine, fixture harness.

---

## 9. Open questions for the developer

1. **`Stream<T>` representation: UFCS-over-`List` (recommended — zero new Value/class, fluent via the
   shipped UFCS) vs a real single-field wrapper class `Stream<T>` (cleaner type identity, but adds a
   class + erasure and risks a `CTy` operand question).** I recommend UFCS-over-List.
2. **`merge` semantics confirmation:** concat-in-arg-order (recommended) vs a fixed round-robin
   `interleave` (or ship both)?
3. **Ship A1 standalone now, or wait and ship A1+A2 together** once the cooperative scheduler exists
   (A2 reuses A1's surface, so A1-now loses nothing)?
4. **Varargs `Stream.of(a, b, c)`** — does varargs exist yet, or ship `Stream.of_list(xs)` +
   2/3-arg `of` overloads (method overloading shipped in M-RT) for now?
5. **`distinct` for object/struct elements** — structural `eq_val` (recommended, matches
   `List.contains`) vs identity? And confirm PHP `array_unique(SORT_REGULAR)` parity on the chosen
   element types (pin the example to scalars first).

---

## 10. Relationship to the rest of the concurrency cluster

A1 is the **operator-algebra layer** and is independent of the scheduler. The digest's keystone —
the *virtual-time / logical-clock cooperative scheduler* (a deterministic ready-queue, min-heap on
`(deadline, insertion_seq)`) — is what A2's lazy streams, async/await, channels, and the actor
runtime all share. **Sequencing recommendation:** ship A1 (free, now) → build the cooperative
scheduler (separate stage) → A2 lazy streams + async/await + channels ride it → live sources (B) ride
serve.rs/Transport. A1 delivers the user-visible "reactive streams" value immediately with zero risk
to the byte-identity spine.
