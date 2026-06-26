# Adversarial Review — "Pure Data-Parallelism (`Core.Parallel`: parallelMap / fork-join)"

**Target:** `docs/research/extended-modules/raw/design-pure-parallelism.md` (Tier A, feasibility 95%).
**Reviewer stance:** refute. Hunt hidden non-determinism, `php -n` walls, and gating leaks.

**Bottom line:** The *feature* is sound — the shipped sequential slice genuinely is `List.map` with a
name, and Tier A is the right call. But the design's **byte-identity GATING claim is materially wrong**,
and two of its determinism arguments are weaker than stated. The verdict is `mixed`: keep Tier A and the
sequential mechanism, but correct the gating story and tighten three claims before adoption.

---

## R1 — [Verified, LOAD-BEARING] The example glob gates only TWO legs, not three. §4/§3 overstate it.

The design's headline argument (§4 + §3) is: *"`examples/guide/parallelism.phg` auto byte-identity-gated
by the `examples/**/*.phg` glob in `tests/differential.rs` … the three-leg spine holds by construction."*

**This is false for the PHP leg.** I read the actual glob test:

- `all_examples_match_between_backends` (`tests/differential.rs:991`) calls `agree(&src)` per example.
- `agree` (`tests/differential.rs:50`) does **only** `cmd_run` vs `cmd_runvm` — `run ≡ runvm`. It never
  transpiles and never runs PHP. (Verified: `sed -n '991,1021p'` of the test body contains **no**
  `transpile` / `run_php` / `PHORGE_REQUIRE_PHP` reference.)
- The PHP oracle lives in `agree_out_php` (`tests/differential.rs:378`), which is invoked only from a
  **hand-curated list** of `#[test]` functions (lines 404, 422, 438, …), each passing an explicit
  `expected` string and label. It is *not* wired to the example glob.

**Consequence:** shipping `examples/guide/parallelism.phg` gates `run ≡ runvm` automatically, but the
**third leg (real `php -n` 8.5) is NOT exercised** unless the author *separately* writes an
`agree_out_php` test for the parallel constructs. The design's §9 deliverables list says only
"differential cases" in passing and the §4 byte-identity argument explicitly leans on the glob. A
reviewer following §9 literally would ship a feature whose PHP transpile is **unverified by CI**.
This is the single biggest defect: the "by construction, all three legs" claim is not delivered by the
mechanism the design names. **Fix:** make a dedicated `agree_out_php` (or equivalent PHP-oracle) test
for `Parallel.map`/`forkJoin` a hard, enumerated deliverable — do not rely on the glob for the PHP leg.

(Note: this is a pre-existing property of the harness, shared by every example. But the design uniquely
*rests its Tier-A safety argument* on the false premise that the glob is three-legged.)

## R2 — [Verified] The shipped sequential slice is genuinely byte-identical run≡runvm≡PHP. Tier A is correct.

Refutation attempts that FAILED (the feature survives them), confirming Tier A for the shipped code:

- **`list_map` is exactly as described** (`src/native/list.rs:189`): a sequential `for x in xs` pushing
  `call(f, [x.clone()])?`, returning `Value::List` in input order; `pure: true`; `HigherOrder`; erases
  to `array_map($f, $xs)` (`src/native/list.rs:292`). A `Parallel.map` near-copy inherits all parity
  properties.
- **`array_map` evaluates left-to-right with side effects in order** (verified under `php -n` 8.5:
  `array_map(function($x){ echo "side:$x\n"; … }, [1,2,3])` printed `side:1/2/3` then `1,4,9`). So even a
  body that calls `Console.println` (which erases to `echo`) preserves order on the PHP leg, matching the
  sequential Rust loop. The §5.1 "println allowed" claim holds **for the shipped sequential slice**.
- **`forkJoin`'s PHP target round-trips** (verified under `php -n` 8.5): `array_map(fn($t)=>$t(),
  [fn()=>101, fn()=>401, fn()=>901])` → `101,401,901`; with side-effecting thunks, output order is
  preserved (`a/b/1,2`). The `forkJoin` erasure is real, not hand-waved.
- **No `php -n` missing-ext wall.** `array_map`/`array_reduce`/`array_values`/`array_filter` are PHP
  *core* (not mbstring/BCMath/gmp/APCu). No TLS, no socket, no Fiber. The §6 "today sequential, std-only,
  zero deps" claim is correct — `Rc` never crosses a thread because there is no thread.

So the **substantive feature is not refutable as a today-breaker.** `determinism_holds = true` for the
shipped sequential slice; `feasible_std_only = true`.

## R3 — [Inferred, real but deferred] §5's `pure`-flag-as-purity-proxy has a hole the design doesn't name: a *seeded* PRNG.

§5.1 defines the parallel-safety deny-list as "natives with `pure == false`" and equates
`pure == false` with "the only ambient-nondeterministic surface." But `pure` means *deterministic w.r.t.
the program text* (verified in the `NativeFn::pure` doc comment, `src/native/mod.rs`), **not**
*order-independent*. The project brief explicitly plans a **seeded `Core.Random` that is deterministic
(`pure: true`)**. A shared-state seeded PRNG drawn inside parallel bodies is `pure:true` (text-
deterministic) yet **order-sensitive**: under the future physical backend, the sequence each task
observes depends on scheduling, even though every individual native is "pure." The purity walk would
**wave it through** (it only rejects `pure:false`), then the physical backend would diverge.

This is not a today-breaker (sequential is fine) and the PRNG doesn't exist yet — but it **falsifies the
design's stated equivalence** "parallel-safe body ⟺ no `pure:false` reach." The correct invariant is
*no reach to a stateful native* (PRNG, future shared counter), which is a strictly larger set than
`pure:false`. The deny-list must be `pure:false` **plus** any future stateful-but-deterministic native,
or the physical backend's safety proof is invalid. **The design should name this and reserve the wider
deny-list now**, since `Core.Random` is already on the roadmap.

## R4 — [Verified, minor] §9 understates the `E-PARALLEL-CAPTURE` work: `free_vars` returns names, not mutability.

§5.1 + §9 imply the capture guard "reuses `ast::free_vars`." Verified `ast::free_vars`
(`src/ast/walk.rs:17`) returns `Vec<String>` — **names only**, no mutability classification. To enforce
"immutable captures only" the checker must additionally resolve each captured name to its binding and
read its `mutable` flag (`VarDecl.mutable`, `src/ast/mod.rs:425`). Feasible, but it is *not* a free
reuse of `free_vars` — it's a name-resolution pass against the enclosing scope. The "afternoon,
mechanically" framing (§9) is optimistic for the checker portion; the Medium estimate survives but the
"reuse free_vars" shorthand is misleading.

## R5 — [Verified] §8's `reduce` honesty problem is real (the design itself flags it). Recommend dropping `reduce`.

`Parallel.reduce` shipped as a strict left-fold *is literally `List.reduce`* with zero parallelism
benefit (the fold is sequential by construction). The design's own §11-Q1 + §8-1 concede this and
recommend Option A (drop `reduce`, ship only `map`+`forkJoin`). I concur and **strengthen it to a
requirement**: shipping `Parallel.reduce` as a left-fold violates the philosophy-of-Phorge "no
surprises" rule (a name implying a benefit it cannot deliver). It should not ship.

## R6 — [Verified] Quarantine question is N/A; no leak risk. Tier-A airtightness is moot but the gate is name-based.

Because the natives are `pure:true`, `uses_impure_native` (`tests/differential.rs:916`,
substring-matches `import {impure_module}`) will **not** skip the example — correct, it *should* be
gated by the two legs it covers. There is no quarantine to make airtight (that's the point of Tier A).
Worth noting the impure gate is `src.contains("import Core.Process")` style name-matching; if
`Core.Parallel` were ever (wrongly) marked `pure:false`, the example would be silently skipped and lose
even the `run≡runvm` gate — so the `pure:true` marking is load-bearing for gating, not just semantics.
Not a defect of this design, but a constraint to state.

---

## Verdict

- **Tier:** A — **upheld** for the shipped sequential slice. The feature is `List.map`-with-a-permission-
  slip; nothing in the shipped code introduces non-determinism. Do **not** downgrade to B.
- **`determinism_holds`:** **true** (for the shipped sequential implementation; the future physical
  backend's determinism is contingent on R3's wider deny-list, which is deferred work, not this slice).
- **`feasible_std_only`:** **true** (sequential native is pure std, zero deps; verified no `php -n` wall).
- **Revised tier:** `mixed` — the *feature* is solidly Tier A, but the *design document* contains a
  load-bearing false claim (R1) and an unstated determinism gap (R3) that must be corrected before the
  plan is executed. It is not a clean accept and not a reject.

**Required corrections before adoption:** (1) R1 — add an explicit PHP-oracle (`agree_out_php`) test as a
named deliverable; the glob does NOT gate the PHP leg. (2) R3 — widen the purity deny-list invariant from
`pure:false` to "no stateful native reach" and reserve it now (seeded `Core.Random` is on the roadmap).
(3) R5 — drop `Parallel.reduce`. (4) R4 — correct the §9 effort note: capture-mutability needs a name-
resolution pass, not bare `free_vars`.
