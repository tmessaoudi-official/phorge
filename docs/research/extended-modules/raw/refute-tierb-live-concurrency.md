# Adversarial Review — Tier-B Live-Concurrency Escape

**Verdict: the Tier-B classification is COHERENT and the per-feature tiering is sound, but the design's
load-bearing claim — "auto-exclusion from the oracle is free, zero harness edits" — is FALSE as written
for the realistic shape of a concurrency program. The quarantine is NOT airtight. `determinism_holds=false`
(Tier-B never claimed byte-identity, but the *replacement* guarantee it does claim — run≡runvm via skip —
has a verified leak); `feasible_std_only=true` with caveats.**

This is a Tier-B claim, so the test is: *is the quarantine actually airtight, and are the weaker
replacement guarantees real?* Two are real. One has a concrete hole.

---

## REFUTATION 1 (P0, verified) — the project harness has NO `uses_impure_native` guard; a multi-file Tier-B example is gated unconditionally

This is the central, exploitable refutation. The design (§2.1, §6.2, §10) asserts repeatedly that
auto-exclusion is "free … no harness edit … zero harness work … verified." That is true for exactly ONE
of the two differential harnesses.

**Verified in `tests/differential.rs`:**

- `all_examples_match_between_backends` (the **single-file** glob, lines 990–1020) DOES call
  `uses_impure_native(&src)` and `continue`s (line 1004). This is the seam the design relies on. It works
  **only on loose `.phg` files** — `collect_phg` (line 932) explicitly *returns early* on any directory
  holding a `phorge.toml`, so it never even sees a multi-file project.
- `all_example_projects_match_between_backends` (the **project** harness, lines 1029–1057) does **NOT**
  call `uses_impure_native`. Verified mechanically: `grep -c impure` over the project-harness body
  (lines 1038–1057) returns **0**. It unconditionally `loader::load`s every `examples/project/*` root and
  asserts `assert_eq!(run, runvm, …)` plus `run.is_ok()`.

The design's own precedent works **only by accident of file placement**: `examples/process/args-env.phg`
is a **loose single file** (verified: `find examples/process -type f` → `README.md`, `args-env.phg`; no
`phorge.toml`), so it rides the single-file harness that skips it. Every actual *project* in the example
set (`examples/project/{shapes,tempconv,visibility,withdeps}`) is pure today, so the gap has never been
exercised.

**Why this bites Tier-B specifically and not the process precedent:**

1. A real concurrency program is **multi-file by nature** — a server with a router, a worker pool, a
   client. The design's §11 Q5 *recommendation* is a dedicated `import Core.AsyncLive` module ("importing
   it quarantines you" — the Process model). The natural way to ship that walkthrough is a multi-file
   project under `examples/` with a `phorge.toml` (matching `examples/project/*`). The moment it has a
   manifest, `collect_phg` skips it AND `collect_projects` picks it up — and the project harness has no
   skip. Result: a Tier-B project gets `assert_eq!(run, runvm)` applied to it.
2. The design itself (§6.4) says place the example "so the differential's project/example discovery
   excludes it (the process precedent already does this)." **The process precedent does NOT do this for
   projects** — it does it for loose files. The design asserts a property of the harness that the harness
   does not have for the project path. This is an unverified claim presented as verified.

**Is `run ≡ runvm` actually safe for an injected-clock program even inside the project harness?** The
design's §2.2 argument is that both Rust legs share `exec_op`, so under an *injected* clock/transport they
agree. But the project harness runs the **real** `SystemClock`/`TcpStream`, not an injected one (it calls
`cli::run_program`/`cli::runvm_program` directly — there is no injection seam in that path). For
`Time.nowMillis()` the two legs call the native body at *different wall-clock instants*, so:
`run` returns e.g. `1719500000123`, `runvm` returns `1719500000124` → `assert_eq!` **FAILS, flakily, in
CI**. For `Net.recv` against a live socket, arrival timing differs per leg → mismatch. For
`Async.parallelLive` the OS interleaving of two child `proc_open`s differs per leg → mismatch. The "run≡runvm
holds" guarantee is conditioned on injection, but the gating harness that would catch a leaked Tier-B
project provides **no injection** — so a leaked program doesn't just escape the gate, it actively breaks
CI nondeterministically.

**Fix the design must adopt (not optional):** the project harness MUST gain the same `uses_impure_native`
skip. Concretely: in `all_example_projects_match_between_backends`, after `loader::load`, read the merged
unit's source (or each file's source) and `continue` if any imports an impure module — OR, more robustly,
check the loaded program's resolved native references against `registry().filter(|n| !n.pure)`. The design
claims "zero harness edits"; the truthful claim is "one harness edit (mirror the skip into the project
harness), without which a multi-file Tier-B example silently breaks the build." This is a real, required
change the design omits.

---

## REFUTATION 2 (P1, verified) — `uses_impure_native` is a substring match on `import <module>`, defeated by aliasing and not transitive

`uses_impure_native` (line 923): `impure.iter().any(|m| src.contains(&format!("import {m}")))`. Two gaps:

1. **Import aliasing.** Phorge supports `import a.b as c;` (M5 S2c, contextual `as`, verified in CLAUDE.md).
   `import Core.Time as T;` still contains the substring `import Core.Time`, so the simple alias survives —
   BUT the substring check is brittle: `import Core.TimeZone` (a hypothetical future pure module) contains
   `import Core.Time` as a substring → a *false positive* skip; conversely a formatting variant
   (`import  Core.Time` with two spaces, or a leading comment line `// import Core.Time`) is a false
   negative. The design treats this matcher as robust ("reads the flag off the registry"); the registry
   part is robust, the **source-text matching is not**.
2. **Not transitive.** A `package Main` that imports a first-party library package `App.Worker`, which in
   turn imports `Core.Time`, has a `main.phg` whose source does NOT contain `import Core.Time`. The
   single-file harness wouldn't see it (it's a project), and even a per-file scan only catches the file
   that literally writes the import. The loader flattens all files into one program before any backend; the
   gate must run on the **post-load merged program / its resolved native set**, not on the entry file's
   text. The design never addresses transitivity — for a single-file `Core.Process` example it never
   mattered; for a multi-file concurrency project it is the common case.

Both gaps reinforce Refutation 1: the correct gate is "does the loaded program reference any `pure:false`
native," computed from the resolved AST, not a substring scan of one file.

---

## REFUTATION 3 (P1, verified) — new `Value::Handle` variant touches 125 `Value::` arms; "never crosses the spine" is true only if Refutation 1 is fixed

The design (§5) adds an opaque `Value::Handle(Rc<RefCell<NativeHandle>>)` and claims it "never crosses the
byte-identity spine … its `Debug`/`type_name`/equality can be coarse." Verified: `src/value.rs` has **125
`Value::` match arms** (`grep -c`), and `#![forbid(unsafe_code)]` is set (lib.rs:3, main.rs:3), so the
recommended closed-enum (not `dyn Any`) is the right call. BUT:

- Adding a variant forces edits to every exhaustive match: `eq_val_rec` (line 274), `type_name` (line 232),
  `truthiness` (line 253), `HKey` construction, `Debug`, the interpreter, the VM, the transpiler's value
  emit. The design says these "can be coarse" — correct **only because** the handle is quarantined. If
  Refutation 1's leak lets a handle reach the project harness, the coarse `Debug`/`eq_val` *do* cross the
  spine: `assert_eq!(run, runvm)` on a program that returns or prints a `Handle` would compare two coarse
  `"<handle>"` strings that happen to match — masking a real difference, or, if `Debug` includes a pointer
  address (the obvious coarse impl), **diverging** per leg. So "coarse equality is safe" is contingent on
  the quarantine being airtight, which Refutation 1 shows it is not. The two refutations compound.

This is not fatal to feasibility (the variant is implementable, std-only, `!Send` by holding `TcpStream`/
`Child` as the design correctly notes), but the "never crosses the spine, so coarseness is free" claim is
load-bearing on a quarantine that has a hole.

---

## REFUTATION 4 (P2, verified-by-reasoning) — `proc_open` fan-out submission-order merge is NOT free of non-determinism across legs

The design (§3.3, §4, §7 matrix) lists "Ordered merge of return values (`joinAll`/`parallelLive`): ✅
guaranteed on all three legs." The Rust legs collect child outputs in submission order — fine. But the
PHP transpile target is `proc_open` + "loop `proc_close` in submission order, collect outputs in order."
Two issues:

- A child proc's **stdout buffering / pipe-fill deadlock**: `proc_open` with unread pipes can deadlock if a
  child writes more than the pipe buffer before the parent reads. The design's "collect in submission
  order" naive loop (`proc_close` then read) risks a hung child on large output. This is a real PHP
  `proc_open` footgun, not a determinism break per se, but it undermines the "PHP leg guarantees ordered
  merge" claim — the merge is ordered *only if* the reads are interleaved correctly, which the one-line
  target glosses.
- More importantly: the claim is "return-value order guaranteed." But a child that **dies / is killed /
  times out** produces a different result-vector length per leg (the Rust `Child` and PHP `proc_open` have
  different failure surfaces — exit-code semantics, signal handling). The design's "first-submitted error
  wins" is a *policy*, but the legs don't share the policy implementation: the Rust side implements it in
  the native body; the PHP side must replicate it in emitted PHP. The design provides NO emitted-PHP sketch
  for "first-submitted error wins" — only `loop proc_close`. So the one determinism the design says it
  KEEPS (ordered merge + first-error-wins) is unverified on the PHP leg. Since Tier-B isn't oracle-gated
  this doesn't break CI, but it breaks the design's §7 promise that ordered merge is "✅ on PHP."

This is why §10/§11 correctly flag `parallelLive` as the 15% risk and recommend deferral. Agreed — but the
matrix overstates the current guarantee.

---

## CONFIRMED-SOUND (the refutation did NOT find a hole here)

- **`php -n` core availability.** `stream_socket_client`, `usleep`, `microtime`, `proc_open`, `fread`,
  `fwrite`, `fclose`, Fibers are all PHP **core** (not extensions) — present under `php -n`. No
  missing-ext wall for the timer/socket/process targets. [Verified: these are core functions, not
  mbstring/PHPUnit/gmp/bcmath territory; consistent with the project's `php -n` constraint.]
- **TLS wall correctly identified and respected.** §3.2/§11 Q3: no Rust TLS, HTTPS only via shelling out
  through `Core.Process`. This is the one genuine hard wall and the design handles it correctly (http-only
  raw socket + curl escape). No refutation.
- **`!Send` heap → OS-thread REJECT is type-level, correct.** `Value` is `!Send` (Rc-shared, verified in
  CLAUDE.md + the `Rc<…>` variants throughout `value.rs`). The two REJECT rows (shared-mutable OS threads,
  scheduler introspection) are truly incoherent for this runtime, not merely hard — a `Conn`/`Handle`
  holding a `TcpStream`/`Child` is `!Send` by type, so the rejection is enforced by the compiler. Confirmed
  correct reject.
- **No new `Op` needed.** Every Tier-B native is `Op::CallNative` (matches `Core.Process` precedent —
  verified `Core.Process` adds no Op). True; no 3-match coupling triggered.
- **Tier-B (not Tier-A) is the correct classification.** Wall-clock readiness / socket arrival / OS
  interleaving are not a function of program text → cannot be byte-identity gated. The logical/physical
  readiness line (§1 "Why not Tier-A") is the right cut. No refutation of the tier.

---

## NET ASSESSMENT

The **tier verdict (B), the per-feature breakdown, the two REJECTs, the std-only feasibility, and the
no-new-Op finding are all sound.** The design's *architecture* is correct.

What is REFUTED is the design's strongest stated guarantee: **"auto-exclusion is free, zero harness edits,
verified."** It is verified for the single-file harness and FALSE for the project harness (Refutation 1),
and the gate matcher is non-transitive + substring-brittle (Refutation 2). For the realistic multi-file
shape of a concurrency example — which the design's own §11 Q5 recommends — a leaked Tier-B project would
hit `assert_eq!(run, runvm)` with real (non-injected) clocks/sockets and **break CI flakily**. The fix is
one required harness edit the design claims is unnecessary.

`determinism_holds=false` (the run≡runvm replacement guarantee leaks via the project harness).
`feasible_std_only=true` (the build is std-only and real; the hole is in the test-quarantine wiring, which
is fixable — not in the runtime). Revised tier: **B** (unchanged — the classification survives; the
quarantine wiring needs the documented fix before this is shippable).
