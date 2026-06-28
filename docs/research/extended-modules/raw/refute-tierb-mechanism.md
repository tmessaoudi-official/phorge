# Adversarial Refutation — "Tier-B Impurity Mechanism + Per-Feature Framework (CASE-BY-CASE)"

**Stage 2b — Adversarial review.** Reviewer: extended-modules refutation agent. Date: 2026-06-27.
Target: `docs/research/extended-modules/raw/design-tierb-mechanism.md` (tier=mixed, feasibility=90%,
"the mechanism already ships, the framework is a recipe over the existing seam + G1/G2").

Verdict up front: the **core claim is sound** — the `pure:false` + `uses_impure_native` quarantine seam
genuinely exists and works (verified `src/native/process.rs`, `tests/process.rs`,
`tests/differential.rs:916`). But the design overstates airtightness in **three concrete, code-verified
ways**, and one Tier-A concurrency claim hides a real determinism gap. `determinism_holds=false` and
`feasible_std_only=false` because of R1 and R2 below — both are real holes in the *shipped* harness that
the design's recipe would walk straight into.

---

## R1 — [HIGH, VERIFIED] The project-aware oracle has NO impure skip. A multi-file impure walkthrough breaks the build.

The design (§2 step 6, §3.2/§3.3/§3.1) prescribes shipping each impure feature with an
`examples/<feature>/` *walkthrough*. The M5 convention for any non-trivial walkthrough is a **`phorj.toml`
project** — four already ship (`examples/project/{shapes,withdeps,tempconv,visibility}/`,
verified by `find examples -name phorj.toml`).

The single-file glob test `all_examples_match_between_backends` DOES skip impure programs
(`differential.rs:1004` → `if uses_impure_native(&src) { continue; }`). **But the project-aware test
`all_example_projects_match_between_backends` (`differential.rs:1030`) does NOT** — verified by reading
lines 1030–1057: it loads every project via `loader::load`, then asserts `run.is_ok()` AND `run == runvm`
**unconditionally**. There is no `uses_impure_native` call anywhere in that function.

Consequence: the moment any impure feature ships its walkthrough as a `phorj.toml` project (the natural
M5 shape), it is gated `run == runvm` with no quarantine escape. Whether that gate passes depends entirely
on R2.

The design's own §0 table cites "SKIP wiring at `tests/differential.rs:1004, 1903`" — but line 1903 is in
the integration/CLI region, not the project harness, and the project harness at 1030 was simply not
audited. The "auto-quarantined with no harness edit" guarantee (§0, §2 step 1) is **false for projects**.

**Fix the design must adopt:** either (a) extend `uses_impure_native` to the project harness (walk every
file in the loaded `Unit`, skip if any imports an impure module — note the current fn only inspects a
single `src` string, so it needs a project-aware variant), or (b) mandate impure walkthroughs be
single-file flat examples under `examples/<feature>/foo.phg` (caught by the glob skip) and NEVER a
`phorj.toml` project. `examples/process/` is currently flat (`args-env.phg`, verified) — so the precedent
the design leans on accidentally dodges this, which is *why the gap was never hit*. The design generalizes
from a precedent that happens to sidestep the exact trap it would introduce.

---

## R2 — [HIGH, VERIFIED] "run≡runvm always survives quarantine" is FALSE for any wall-clock / ambient read in a project, because the two backends run as separate sequential invocations.

This is the load-bearing claim of the whole framework (design §1 "Tier B's Rust legs stay gated against
each other because they share the one `eval` body and one process") and §3.5 ("the interpreter and VM each
build their `Effects` from the same run config, so a frozen-clock test gives identical bytes on both Rust
legs").

The claim holds ONLY when the impure input is a **fixed config** (a seed, a fixture path) and the test
*explicitly* sets it. It does NOT hold for genuinely ambient, time-varying state. Verified mechanism:

- `cmd_run`/`cmd_runvm` (`src/cli/mod.rs:422`/`431`) and `run_program`/`runvm_program`
  (`src/cli/mod.rs:456`/`467`) are **two separate function calls**. The project harness runs
  `let run = cli::run_program(&unit);` then `let runvm = cli::runvm_program(&unit);` (lines 1043–1044) —
  sequentially, microseconds apart.
- A `Core.Time.now()` that reads `SystemTime::now()` (or the design's G1 `Effects.now()` built from the
  *real* clock when no fixture freezes it) returns time **T1** during `run_program` and **T2 ≠ T1** during
  `runvm_program`. → `run != runvm`. → the project harness assert at line 1050 FAILS.

The design half-acknowledges this in §3.5 ("*Do not* read `SystemTime::now()` directly... always go
through the injected handle so a test can freeze it") — but freezing only happens in the *dedicated
`tests/<feature>.rs` fixture*. In the **un-skipped project harness** (R1) nothing freezes anything. So R1
and R2 compound: a clock/random *project* walkthrough is both un-skipped AND non-`run≡runvm`-identical.

Even for a *flat* impure example (correctly skipped from the oracle by R1's glob), the design's §3
fixture-test recipe (`tests/<feature>.rs`) for clock/random must inject and freeze the `Effects` handle
through BOTH `cmd_run` and `cmd_runvm` with the *same* frozen value — but `cmd_run`/`cmd_runvm` take only
`&str`, with no parameter to thread an `Effects` config. **G1 as described has no plumbing path**: there is
no argument on the public run entry points to inject the handle. The design says G1 is "tens of lines, no
new core machinery" — but threading a per-run `Effects` through `cmd_run`/`cmd_runvm`/`run_program`/
`runvm_program` (and the deep-stack worker `on_deep_stack`, and `Vm::new`, and the interpreter entry) is a
signature change across the entire run surface, not a localized 4th enum arm. The enum arm is the easy 10%;
the capability-injection plumbing is the uncosted 90%. Contrast `ClassTables`/`ClosureInvoker`: those are
built *inside* the existing backend entry from the already-present `Program` — they need no new caller-side
parameter. `Effects` carrying a *test-supplied frozen clock/seed* fundamentally does, because the test is
the only place that value exists.

This is the analogy break the design relies on and gets wrong: HigherOrder/Reflective inject capabilities
**derivable from the program**; G1 injects capabilities **derivable only from the run environment / test
harness**, which is a different and larger change.

---

## R3 — [MEDIUM, VERIFIED] The cooperative-scheduler "Tier A, ordering is a total language rule" claim is only as deterministic as the SLOWEST-resolving primitive feeding it — and several proposed feeders aren't pure.

Design §4 asserts every cooperative-concurrency primitive is Tier A "by construction" because ordering is a
FIFO/source-order/insertion-seq language rule. The ordering *rule* is indeed deterministic. But the
scheduler is byte-identical across legs only if **every task body it runs is itself deterministic**. The §4
table lists `parallelMap`, actors, channels as Tier A — correct *while the bodies are pure*. The trap: the
same table also normalizes `Core.Time`/`sleep`-driven and live-socket primitives to Tier B, but the
scheduler is a *shared substrate*. A single program mixing a Tier-A `Async.group` with one Tier-B
`time.After` task makes the *whole program's* completion order clock-dependent → the program is Tier B, but
nothing in the `uses_impure_native` substring check detects "uses the scheduler with a clock task" vs "uses
the scheduler with pure tasks" — both import the same `Core.Async` module. If `Core.Async` is marked
`pure:false` to be safe, then EVERY scheduler program (including the genuinely-pure `parallelMap`) is
quarantined and loses its gated example — contradicting the §4 "Tier A, gated" claim. If `Core.Async` is
`pure:true`, the clock-mixing program leaks into the oracle. The `pure` flag is **per-native (per-module)**,
but determinism here is **per-call-graph**. The design has no mechanism to resolve this; it needs the
clock/sleep natives to be a *separate impure module* (`Core.Time`) so the substring check fires on the
clock import, not the scheduler import — which works ONLY if the logical-clock `sleep` (Tier A) and
wall-clock `sleep` (Tier B) are in *different modules*. The design puts both under "scheduler virtual time"
vs "`Core.Time.now()`" but doesn't commit to the module split that makes the quarantine detector actually
fire. **This is a real soundness gap in the per-feature triage, not just plumbing.**

---

## R4 — [MEDIUM, VERIFIED] PRNG byte-identity across the THIRD leg (PHP) is asserted but the design admits it can't reuse PHP's RNG — and the float-from-bits step is where it breaks.

Design §3.6 correctly flags that `mt_rand` ≠ any Rust PRNG and mandates a "hand-rolled identical PRNG in
both the Rust kernel and the emitted PHP." The integer-domain xorshift/SplitMix64 claim is plausible (PHP
ints are 64-bit, bitwise ops match). But the design also wants "random doubles" (§5 risk 3 names
"float-from-bits representation" and "random doubles") and a seeded Faker producing realistic values. PHP
has **no native u64**: PHP integers are *signed* 64-bit, and any PRNG step that relies on unsigned
right-shift (`>>>`) or unsigned multiply-overflow wrapping must be emulated in PHP with sign-correction
(`>>` is arithmetic in PHP, and `*` on two large ints silently promotes to float past 2^53). SplitMix64's
`z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9` overflows i64 and in PHP becomes a **float** (lossy past 2^53)
unless rewritten with GMP — and **GMP is ABSENT under `php -n`** (stated in the prompt's env). So the
"hand-rolled identical PRNG" is feasible in the Rust legs but **not bit-reproducible in core-only PHP 8.5**
without 64-bit unsigned wrapping, which `php -n` core cannot do losslessly. This drops seeded-random from
"Tier A, byte-identical across all three legs" (design's keystone claim) to **Tier A run≡runvm only**,
PHP-quarantined — i.e. it is actually Tier B by the design's own definition. The design's feasibility 80%
and "seeded ~90% byte-identical three legs" is too high; the three-leg identity is the part that fails.
(Mitigation exists — pick a PRNG whose state fits in 32 bits so all arithmetic stays under 2^53, e.g. a
32-bit xorshift or PCG-XSH-RR with 32-bit output — but the design didn't constrain the algorithm to the
PHP-representable domain, and `Core.Random` producing only 32-bit outputs is a weaker surface than implied.)

---

## R5 — [LOW, VERIFIED] `uses_impure_native` is a raw substring match — robust for aliases, but has a quiet false-NEGATIVE class.

Verified `differential.rs:923`: `impure.iter().any(|m| src.contains(&format!("import {m}")))`. Positives:
the alias form `import Acme.Label as Fmt;` (shipped in `examples/project/tempconv/src/main.phg:11`,
verified) still contains the leading `import Core.Time` substring, so aliasing an impure import is caught —
good. But: a re-export, or a module that *transitively* pulls an impure native via a same-package call to
another file that imports it, is NOT caught — the check only inspects the single `src` string of the file
being run, not the merged unit's full import set. For single-file examples this is fine; for the project
case (R1) it's a second reason the project harness can't be made safe by naively reusing the single-`src`
function. Low severity (no impure transitive import ships today), but the design's "derived, no harness
edit" guarantee is weaker than stated once multi-file enters the picture.

---

## What survives (the design is RIGHT about)

- The `pure:false` flag + single-file `uses_impure_native` glob skip is genuinely airtight for **flat,
  directly-importing** impure examples. Verified: `examples/process/args-env.phg` is correctly skipped, and
  `tests/process.rs` asserts `cmd_run == cmd_runvm` for the ambient `Process.args()` case (this works
  because `PROCESS_ARGS` is a process-global `RwLock` set by the test BEFORE both calls, so both backends
  read the SAME value — verified `process.rs:14`/`27`). Note this is the *fixed-config* case, exactly the
  one that survives R2. The design's recipe is correct for argv/env-shaped (set-once-then-read) impurity.
- The `serve.rs` `Transport` quarantine is a clean, verified model (`serve.rs:25`, driven OUTSIDE
  `differential.rs`, tested via `tests/serve.rs` in-memory double). Crucially it works BECAUSE the impurity
  lives *outside* the `eval` body (`serve` takes `&Program`, is never a native) — which is the OPPOSITE of
  G1's inside-the-eval-body injection. The design conflates the two as "the same model"; they aren't. The
  Transport model is sound; the G1 model inherits R2's plumbing problem.
- "No new VM `Op`" is correct for everything that rides `Op::CallNative` / `run_until`. The reject list
  (shared-mutable OS threads, mutexes, atomics, preemptive scheduling) is genuinely incoherent under a
  `!Send` `Rc` heap with no preemption point in `exec_op` — verified that confines it to inexpressible, not
  merely hard.

---

## Net assessment

- **`determinism_holds = false`** — R2 (clock/random in the un-skipped project harness breaks `run≡runvm`)
  and R3 (scheduler determinism is per-call-graph, not per-module) are real holes in the *shipped*
  determinism story, not hypotheticals.
- **`feasible_std_only = false`** — R4: seeded-PRNG three-leg byte-identity is NOT achievable in `php -n`
  core for a 64-bit PRNG (no lossless u64), so the keystone "seeded = Tier A across all three legs" claim is
  std-only-infeasible as specified; it degrades to PHP-quarantined (Tier B).
- **Revised tier: mixed** stands — the per-feature split is the right shape and the reject list is correct —
  but the design's *confidence numbers and airtightness claims are too high*: the mechanism is airtight only
  for the narrow set-once-ambient (`Process`/`Env`) and outside-the-eval (`serve`) shapes it generalizes
  from, and breaks for the broader clock/random/scheduler surface it most wants to add.
