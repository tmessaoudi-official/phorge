# STAGE 2b — ADVERSARIAL REVIEW: "Persistent cache (Tier-B)" + `Core.Memo` (Tier-A)

Verdict summary: the design's **two-tier split, Tier-A purity, and per-feature recommendation are
sound**, but its central Tier-B safety claim — that the impure quarantine is *airtight* "by the shipped
`uses_impure_native`" — is **demonstrably false for the multi-file project harnesses**. It is a latent
hole (no Tier-B project example is proposed, so it does not bite *today*), but the design asserts
airtightness without the caveat, which is exactly the kind of unstated assumption that breaks the
byte-identity spine the first time someone ships a realistic "caching app" example as a project. I
therefore set **determinism_holds=false** (the Tier-B quarantine is not the airtight guarantee claimed)
and **feasible_std_only=true** (the std-only Rust feasibility itself is real and verified).

---

## 1. CONFIRMED-SOUND claims (the design got these right)

These survived adversarial checking against the live code this session:

- **Tier-A `Core.Memo` is genuinely pure / byte-identical.** It is a thin COW wrapper over the shipped
  `Value::Map(Rc<Vec<(HKey, Value)>>)` + `value::map_set` kernels; it carries **no static state**, no
  clock, no PRNG, no cross-process surface. [Verified: `src/value.rs:50` `Map` arm; the design's COW
  claim matches the shipped `Core.Map`.]
- **`getOrCompute` "invokes the closure at most once per key in source order" is correct by
  construction.** It would reuse `NativeEval::HigherOrder` + the backend-supplied `ClosureInvoker`, the
  *same* invoker that drives `List.map`/`filter`/`reduce` — `call(f, args)` runs the one body on both the
  interpreter and the re-entrant VM (`run_until`/`call_closure_value`), so a one-vs-two invocation split
  is structurally impossible. [Verified: `src/native/list.rs:189–232` — the invoker is shared; the
  design's risk #4 is real *in general* but mitigated *by the shipped mechanism it reuses*.]
- **No `Default` for `pure` — a missed flag is a compile error, not a silent gate.** Every one of the
  120 `pure:` declarations in `src/native/*.rs` is explicit; `NativeFn` has no `Default`. So the design's
  "declare `pure:false`" step cannot be silently skipped. [Verified: `grep -c "pure:"` = 120; no
  `impl Default for NativeFn`.]
- **The injected-struct return shape does not leak HashMap order.** `Instance.fields` is a
  `RefCell<HashMap<String, Value>>` (`src/value.rs:94`) — a real HashMap — BUT the `MemoResult { value,
  cache }` fields are accessed **by name** (`r1.value`), never iterated, so iteration order is never
  observable. Same pattern as every shipped struct. [Verified: field reads are keyed, not ordered.]
- **Import-alias does not defeat the substring quarantine.** `import Core.Cache as C;` still
  `contains("import Core.Cache")`, so an aliased Tier-B import is still caught by `uses_impure_native`.
  [Verified: `tests/differential.rs:923` does `src.contains(&format!("import {m}"))`; alias syntax
  confirmed in `examples/project/tempconv/src/main.phg:11`.]
- **std-only Rust feasibility is real.** `RwLock`/`HashMap`/`std::fs`/`SystemTime` cover the in-process
  and file backends with zero crates; Redis/APCu live only on the (deferred) PHP leg. No TLS wall is
  load-bearing because the Rust legs degrade to an in-process map. [Verified: `process.rs` already ships
  a `static PROCESS_ARGS: RwLock<Vec<String>>` (`src/native/process.rs:26`) — the exact precedent.]

So **feasible_std_only=true** and the **Tier-A determinism is real**. The refutation is narrower and
sharper than "the design is wrong."

---

## 2. THE REAL HOLE — the Tier-B quarantine is NOT airtight as claimed (P0)

The design's §4.1 / §4.3 / §5 repeatedly ground Tier-B safety in: *"auto-dropped from `differential.rs`
by the shipped `uses_impure_native` (which reads the `pure` flag off the registry)."* That is true for
**two** of the **four** relevant harnesses and **false** for the other two.

| Harness | Line | Calls `uses_impure_native`? |
|---|---|---|
| single-file `run≡runvm` glob (`all_examples_match_between_backends`) | `tests/differential.rs:1004` | **YES** (skip-before-run) |
| single-file PHP oracle (`all_examples_transpile_and_match_php`) | `tests/differential.rs:1904` | **YES** |
| **project `run≡runvm`** (`all_example_projects_match_between_backends`) | `tests/differential.rs:1030` | **NO** |
| **project PHP oracle** (`all_example_projects_transpile_and_match_php`) | `tests/differential.rs:1938` | **NO** |

[Verified: read all four bodies. The two project harnesses loop `for project in &projects { … }` and
assert `run == runvm` / `php == interpreter` **unconditionally** — there is no `if uses_impure_native`
guard in either.]

Why this matters specifically for a *cache* (and did not for `Core.Process`/`Core.Env`):

1. **`agree()` runs both backends back-to-back in ONE process.** `tests/differential.rs:50` does
   `cmd_run(&src)` then `cmd_runvm(&src)` and asserts equality. A Tier-B `Core.Cache` backed by a
   `static CACHE: RwLock<HashMap<…>>` (the design's recommended Rust impl, §4.3 option 1) is
   **process-global**. The interpreter run populates the static; the *subsequent* VM run sees the
   already-warm cache → `Cache.get` **hits on `runvm` where it missed on `run`** → guaranteed
   `run ≠ runvm` divergence. The single-file glob avoids this by skipping *before* `cmd_run` ever
   touches the static. **The project harness does not skip — so a Tier-B cache shipped as a project
   would fail `run≡runvm` from its own static-state bleed**, the textbook reason the feature must be
   quarantined.
2. **The project PHP oracle (line 1938) compares PHP against the interpreter** — and a persistent cache
   is non-deterministic w.r.t. the source (cross-process state + TTL clock), the very property §4.1
   names. Unquarantined, a `Core.Cache` project would fail this oracle non-deterministically (green when
   the cache happens to be empty, red on a warm re-run).

The design proposes (§5) only an `examples/cache/` *walkthrough README* (not a runnable project) and a
Tier-A `examples/guide/memo.phg`, so **the hole is latent, not currently triggered**. But: (a) the
"faults-can't-be-an-example" rule does NOT forbid a *successful* cache program — a `Cache.set` /
`Cache.get` round-trip produces clean `Ok` output and is a very natural example to ship; (b) the design
explicitly claims airtightness "**no harness change needed beyond the `pure:false` flag**" (§4.3), which
is the false statement. **The correct claim is: the `pure:false` flag is sufficient ONLY for single-file
examples; a Tier-B native that ships any multi-file project example requires either extending both
project harnesses with the `uses_impure_native` guard, or a standing rule that Tier-B examples are
single-file-only.** The design omits both.

Required fix to make the design's own claim true: add `if uses_impure_native(&src_of_each_file)
{ continue; }` to `all_example_projects_match_between_backends` and
`all_example_projects_transpile_and_match_php` — but note the project harness loads via `loader::load`
and only reads `main.phg` for the entry; the impure import could live in *any* package file, so the
guard must scan **every** `.phg` under the project root, not just `main.phg`. This is a non-trivial
harness change the design dismissed as unnecessary.

---

## 3. Secondary findings (P1/P2)

- **P1 — fixture-test cross-contamination via the shared static.** `tests/process.rs` resets state with
  `set_process_args(Vec::new())` after each test (`tests/process.rs:28`). The design's `tests/cache.rs`
  MUST mirror this with a `Cache.clear` / static-reset in *every* test teardown, AND `cargo test` runs
  test fns concurrently by default — two cache tests sharing one `static CACHE` will race unless the
  module is `#[serial]` (no such crate — zero-dep) or each test uses a unique key namespace. The design
  names "cross-process state bleed" (risk #2) but understates the **in-process concurrent-test** race,
  which is the more immediate failure given std-only has no `serial_test`. [Inferred: from
  `cargo test`'s default thread-pool + the shared `static` + the zero-dep constraint blocking
  `serial_test`.]
- **P2 — `CLOCK_OVERRIDE` static is itself a global the differential could observe.** A
  `static CLOCK_OVERRIDE: RwLock<Option<u64>>` defaulting to `None` (→ real clock in production) is fine,
  but if a *future* Tier-A native ever reads it, the seam becomes a determinism leak. Document it as
  Tier-B-only. [Speculative — no such cross-use is proposed, but the static is a shared surface.]
- **P2 — APCu-absent runtime selection is correct but untested by the oracle.** The design correctly
  notes APCu is ABSENT under `php -n` and the adapter must runtime-select the file fallback. Because the
  program is quarantined, the oracle **never runs the PHP adapter at all** — so the file-fallback path
  is asserted by *no test*. The design should add a `tests/cache.rs` case that transpiles and runs the
  PHP adapter under `php -n` directly (outside the differential), or the `apcu`-vs-file branch is
  ship-untested. [Verified: APCu absent under `php -n` per project inventory; quarantine means the
  oracle skips it.]

---

## 4. Things that are NOT holes (pre-empting over-refutation)

- **Substring quarantine is not fooled by whitespace/alias** (see §1). NOT a leak.
- **No new VM Op / no new Value** — correct; `Op::CallNative` + opaque `int` handle is consistent with
  the shipped `Op::Print → Op::CallNative` discipline and avoids the value-kernel ripple. NOT a hole.
- **Tier-A is not secretly Tier-B.** `Core.Memo` has no static, no clock; the COW Map guarantees
  per-run determinism. The split is the right cut. NOT incoherent.

---

## 5. Bottom line

- The **reject/mixed verdict and the two-module split are correct** and well-reasoned.
- **Tier-A `Core.Memo` byte-identity holds** — confirmed against the shipped Map kernels + shared
  closure invoker. (If the topic were Tier-A alone, determinism_holds=true.)
- **Tier-B's airtightness claim is false as stated:** the impure quarantine covers single-file examples
  but **both project harnesses bypass `uses_impure_native`**, and a `static`-backed cache cross-
  contaminates `run`→`runvm` within the one test process. The hole is latent (no Tier-B project example
  proposed) but the design's "no harness change needed" assertion is wrong and would silently break the
  first realistic cache project example. → **determinism_holds=false.**
- **std-only feasibility is genuine** → **feasible_std_only=true.**
- Feasibility 85% is slightly optimistic given the unbudgeted harness-guard work + the concurrent-test
  serialization problem (no `serial_test` under zero-dep); ~78% is more honest.
