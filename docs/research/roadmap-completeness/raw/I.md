# Track I — Performance — roadmap-gap audit

## Track summary

Phorj's performance story today is **two correct backends and one honest measurement tool**: a
tree-walking interpreter (the reference), a stack VM (`phg runvm`, ≈3.2× faster than the interpreter
on `examples/bench/workload.phg`, and `--vs-php` shows it beating a debug PHP 8.6), and `phg bench`
(median-of-N wall-clock + cold peak-RSS, Linux `/proc`) gated so a backend disagreement *aborts* the
benchmark. What is conspicuously **absent is any optimization layer**: the bytecode the compiler
emits is exactly what it lowers — `src/compiler.rs` has no const-folding pass, no peephole, no
dead-code elimination, no superinstructions; the `Op` set (`src/chunk.rs:66`) is fully naive
(`AddI`/`GetLocal(n)`/`Const(i)` with no `GetLocal0`, no `AddIConst`, no fused dispatch). The VM
dispatch loop (`src/vm.rs:73`) is a `match op` over a **cloned** `Op` every instruction — and
`Op::IsInstance(String)` clones a heap `String` on every execution, while `Value::Str(String)` is
the one non-`Rc` value variant, so `Op::GetLocal` on a string local deep-copies the string (every
other heap value is a refcount bump since M2 P5a). None of this is *wrong* — it is the deliberate
"earn complexity" posture — but it is the entire forward surface for this track. The v2 horizon
(native AOT, ownership/no-GC, sized ints) is named in ROADMAP/VISION but undesigned. Crucially,
**nothing in CI guards against a perf regression** (`ci.yml` runs fmt/clippy/test/oracle + a
cross-build job; there is no bench job, no recorded baseline, no ratchet), which is the single
highest-leverage, most philosophy-aligned gap: it protects the *measured* 3.2× the project already
advertises. The philosophy lens matters sharply here: most classic VM optimizations are **invisible
to the PHP-familiar developer** (they change neither syntax nor semantics, only speed), so they cost
**zero surprise budget** — but they also must never threaten the byte-identical spine, which makes
"prove the optimized bytecode still matches" the gating constraint, and makes a perf-regression gate
a prerequisite, not a nicety. Sized integer types are the one item here that *is* a visible language
feature (and a real PHP-parity item: PHP has no sized ints either, so this is a beyond-PHP, v2-shaped
addition that must be weighed against the surprise it introduces).

## Gaps

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| I-regress-gate | Perf-regression gate in CI (baseline + ratchet) | new | strong | adopt | M9 | M |
| I-str-rc | `Rc`-share `Value::Str` (kill string deep-clone on `GetLocal`) | port | strong | adopt | M-perf (new) | S |
| I-isinstance-interned | Intern `Op::IsInstance(String)` to an index (kill per-dispatch clone) | port | strong | adopt | M-perf (new) | S |
| I-constfold | Constant-folding compiler pass | new | strong | adopt | M-perf (new) | M |
| I-peephole | Peephole / dead-code-after-`return` elimination pass | new | strong | adopt | M-perf (new) | M |
| I-superinstr | Superinstructions (`GetLocal0`, `AddIConst`, fused compare-jump) | new | strong | defer | M-perf (new) | M |
| I-dispatch | Faster dispatch (no per-op `Op::clone`; ref-dispatch / `Copy` ops) | new | strong | adopt | M-perf (new) | S |
| I-inline-cache | Inline caches for method/field/native resolution | new | ok | defer | M-perf (new) | L |
| I-fn-inline | Function inlining (small/leaf functions) | new | ok | defer | v2 | L |
| I-alloc-stack | Allocation reduction (stack value reuse, small-vec, string arena) | new | ok | defer | M-perf (new) | M |
| I-sized-ints | Sized integer types (`i8`/`i32`/`u32`/…) | new | weak | defer | v2 | L |
| I-decimal | `decimal` / arbitrary-precision numeric type | new | weak | defer | v2 | L |
| I-aot | Native AOT compilation (Cranelift/LLVM/own codegen) | new | weak | defer | v2 | L |
| I-ownership-nogc | Ownership model removing the GC | new | weak | reject | v2 | L |
| I-bench-suite | Bench *suite* + tracked history (beyond one workload) | new | strong | adopt | M9 | S |
| I-bench-php-real | `--vs-php` against PHP with OPcache+JIT (release, fair) | map | ok | defer | M9 | S |
| I-vm-stack-precap | Pre-size VM stack / frame vectors from compile-time max depth | new | ok | defer | M-perf (new) | S |
| I-disasm-cost | Per-op cost/profile annotation in `phg disasm` / a `phg profile` | new | ok | defer | M9 | M |

## Rationale per ADOPT item

**I-regress-gate — Perf-regression gate in CI.** The project *advertises* a measured number (VM
≈3.2×; the README and CLAUDE.md both cite it), but nothing prevents a future commit from silently
eroding it. A CI job that runs `phg bench` on a fixed workload, compares against a committed baseline,
and fails when the VM/interpreter ratio (or absolute VM time) regresses past a tolerance is the
single most philosophy-aligned item in this track: it is *correctness applied to performance* — the
same "prove it, don't claim it" discipline the differential harness applies to output. It is also the
prerequisite that makes every other optimization safe to merge (you can prove a peephole pass made
things faster, not slower). Wall-clock in CI is noisy, so the gate should use a generous tolerance or
an instruction-count proxy; either way it ratchets. Belongs in M9 (engineering hygiene) alongside the
CI work already shipped there.

**I-str-rc — `Rc`-share `Value::Str`.** Every heap value became a refcount bump in M2 P5a *except*
`Value::Str(String)` (`src/value.rs:18`), so `Op::GetLocal` on a string local still deep-copies the
string — the exact cost P5a was created to eliminate, left on the one type strings flow through most
(interpolation, every `Core.Text` result). Changing it to `Str(Rc<str>)` is a small, mechanical,
**semantically invisible** change (strings are immutable in Phorj today; mutation is value-type COW),
must pass the byte-identical spine unchanged, and directly speeds string-heavy programs. Pure upside,
zero surprise.

**I-isinstance-interned — Intern `Op::IsInstance(String)`.** The op carries its class name inline as a
`String` (`src/chunk.rs:202`), and the dispatch loop clones the whole `Op` every instruction
(`src/vm.rs:89`), so a hot `instanceof` in a loop allocates a `String` per iteration. Replacing the
inline `String` with a const-pool index (the pattern every other name-carrying op already uses) makes
the op `Copy`-friendly and removes the allocation. Small, invisible, spine-safe.

**I-constfold — Constant-folding pass.** The compiler emits `Const(2); Const(3); AddI` for `2 + 3`;
folding literal arithmetic / string concatenation at compile time is a textbook pass that changes no
semantics (the checker already guarantees operand types and overflow is already a *checked* fault, so
folding must reproduce the same fault for an overflowing constant expression — a nice correctness
constraint, not a blocker). Invisible to the developer, gated by the spine, and it shrinks bytecode
the disassembler shows. A clean first optimization milestone.

**I-peephole — Peephole / dead-code elimination.** The compiler already *knows* code after `return`
is dead (the `height`-drift comments at `src/compiler.rs:808/842/1095` exist precisely because dead
code confuses stack-height tracking). A small peephole pass (drop unreachable ops after a terminal,
collapse `Jump`-to-next, fuse `Not; JumpIfFalse`) is invisible, spine-gated, and pairs naturally with
const-folding in the same M-perf milestone.

**I-dispatch — Faster dispatch (no per-op clone).** The loop clones each `Op` before matching
(`src/vm.rs:87-89`) to dodge a borrow conflict with the mutable stack. Once the two `String`-carrying
ops (`IsInstance`, and any future) are interned to indices, `Op` becomes `Copy` and the clone becomes
a cheap bit-copy or can be eliminated by restructuring the borrow. This is the enabling refactor for
the whole track and is invisible + spine-safe; adopt early in M-perf.

**I-bench-suite — Bench suite + tracked history.** Today there is exactly one workload
(`examples/bench/workload.phg`). A small *suite* (scalar-arithmetic, object-heavy, string-heavy,
recursion, collection ops) gives the regression gate something representative to ratchet against and
turns "we think the VM is fast" into per-domain evidence. Cheap to add (more `.phg` files + a runner),
and it makes I-regress-gate trustworthy rather than a single-point measurement. M9.

---

### Why the DEFER / REJECT calls (brief)

- **I-superinstr / I-inline-cache / I-alloc-stack / I-vm-stack-precap / I-fn-inline** are all
  legitimate, philosophy-aligned (invisible, spine-safe) optimizations — but they should land *after*
  the regression gate + a bench suite exist to prove they help, and after the cheap wins (str-Rc,
  interning, const-fold, peephole). They are deferred on *sequencing*, not merit. Superinstructions
  and inline caches are the higher-ceiling items once the VM's allocation/clone costs are paid down.
- **I-sized-ints / I-decimal** are visible *language* features and genuine beyond-PHP items (PHP has
  neither). They carry real surprise budget (a PHP dev does not expect `i32` wraparound vs Phorj's
  checked-overflow promise — they would *interact* with the "no silent wraparound" invariant and need
  a careful design) and they only pay off under native AOT. v2, where ROADMAP already files them.
- **I-aot** is the v2 anchor (native ahead-of-time). Massive scope, needs a codegen strategy decision
  (own / Cranelift / LLVM — the last two break the std-only/zero-dep invariant, a real tension worth
  flagging now). Deferred to v2 by design.
- **I-ownership-nogc — reject (as currently framed).** ROADMAP lists "an ownership model that removes
  the GC" for v2, but Phorj has *no tracing GC today* (the `Rc`/`Drop` model reclaims the
  immutable+acyclic heap fully; the only leak is mutation-created cycles, an accepted Fork-3 non-goal).
  A Rust-style ownership/borrow model is a maximal PL-theory feature that would massively expand the
  surprise budget for a PHP-familiar developer — the antithesis of the philosophy. Recommend
  *rejecting it as a language-visible feature* and reframing the v2 goal as "a cycle collector *if*
  long-lived-cycle need appears" (already the stated stance) — i.e. the gap is real in the ROADMAP
  text but the right answer is to *narrow it*, not build it.
- **I-bench-php-real / I-disasm-cost** are useful measurement refinements (fair PHP comparison with
  OPcache+JIT on a release build; per-op cost annotation or a `phg profile`) — nice-to-have tooling,
  deferred to M9 behind the regression gate which is the load-bearing one.

## Critic pass

**Verification of the original list (all confirmed against source, not memory):**
- `Value::Str(String)` is the **only** non-`Rc` heap value variant (`src/value.rs:18`; every other
  composite — `List`/`Map`/`Set`/`Instance`/`Enum`/`Closure` — is `Rc<…>`). → I-str-rc is real.
- `Op::IsInstance(String)` is the **only** `String`-carrying op (`src/chunk.rs:202`); every other
  name-bearing op uses a `usize` index. → I-isinstance-interned is real.
- The dispatch loop does `let op = code[ip].clone();` every instruction (`src/vm.rs`, "Clone the op
  (cheap)"). → I-dispatch is real.
- No `const.fold`/`peephole`/`dead-code`/`optimize`/`superinstr`/`inline.cache` symbols exist in
  `src/` (only the dead-code-after-`return` *comments* at compiler.rs:808/842/1095). → the whole
  optimization-pass family is genuinely absent.
- The `Op` set is naive — no `GetLocal0`/`AddIConst`/fused compare-jump (`src/chunk.rs:68–202`). →
  I-superinstr is real.
- ROADMAP/VISION name **exactly** the three v2 perf items already listed (AOT, ownership-removes-GC,
  sized-int) — `ROADMAP.md:84-85`, `VISION.md:77`. No fourth v2 perf item was dropped.

**Mis-listings found: 0.** No item on the original list is already shipped — every adopt/defer
candidate names an optimization or measurement layer that the verification above confirms is absent.
(Note: I-bench-suite and I-regress-gate are correctly listed as *absent* — `.github/workflows/ci.yml`
has no bench/perf job, and `examples/bench/` holds exactly one `workload.phg`.)

**Newly-found items (full rows):**

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| I-range-lazy | Lazy `for`-loop range (don't materialize `0..n` into a `List<int>`) | new | strong | adopt | M-perf (new) | S |
| I-cargo-profile | Release-profile tuning for the VM (`lto`, `codegen-units=1`, `panic=abort` eval) | new | strong | adopt | M9 | S |
| I-intern-symbols | Intern field/method/native names to symbol IDs (kill `HashMap<String,_>` keys) | new | strong | defer | M-perf (new) | M |
| I-op-shrink | Shrink `Op` to a small `Copy` enum after the `String`/`Fault` payloads are out-of-lined | new | strong | defer | M-perf (new) | S |
| I-threaded-dispatch | Threaded / computed-goto-style dispatch (tail-call dispatch fns) | new | ok | defer | M-perf (new) | M |
| I-tco | Tail-call optimization for self-recursion (avoid the 256 MB worker-stack cap) | new | ok | reject | — | M |

Rationale for the new rows:

- **I-range-lazy (adopt, S).** `for (int i in 0..n)` compiles to `MakeRange` which **eagerly
  materializes the full `List<int>`** (`src/vm.rs:317`; `src/compiler.rs:1408-1411`) — an O(n)
  allocation for what is the single most common loop shape. A lazy integer-range iterator for the
  `for` lowering (keep `MakeRange` for the value-position case) is invisible (semantics identical,
  spine-gated) and removes a whole-list allocation from the hottest loop. The researcher's track
  summary mentions the `Op` set but missed that one existing op allocates linearly. Cheap, pure
  upside, philosophy-perfect.
- **I-cargo-profile (adopt, S).** `Cargo.toml` has **no `[profile.release]` block at all** (verified —
  empty grep), so the binary that produces the advertised "VM ≈3.2×" number ships with default
  release settings (`codegen-units=16`, thin-LTO off). `lto="fat"` + `codegen-units=1` is the
  cheapest possible win, completely invisible, and — like I-regress-gate — it makes the *advertised
  number honest*. Belongs with the M9 CI/build hygiene. The most-overlooked free win in the track.
- **I-intern-symbols (defer, M).** Instance fields live in a `HashMap<String, Value>` per instance
  (`src/vm.rs:413`) and method/native resolution is name-keyed. Interning every identifier to a
  `u32` symbol ID at compile time (lookups become array-index / integer-compare) is the structural
  foundation that makes I-inline-cache and I-op-shrink cheap, and shrinks per-instance memory.
  Distinct from I-inline-cache (interning helps *every* lookup, caches only help repeated
  monomorphic sites). Defer behind the bench suite, adopt before the caches.
- **I-op-shrink (defer, S).** Once `IsInstance` is interned (I-isinstance-interned) and `Fault(FaultMsg)`
  is out-of-lined (index into a side table), `Op` becomes a small `Copy` enum — improving instruction
  i-cache density and *enabling* I-dispatch's clone→bit-copy. Listed separately because it is the
  payload-diet step that I-dispatch's "`Op` becomes `Copy`" assertion silently depends on but does not
  itself perform. Invisible, spine-safe.
- **I-threaded-dispatch (defer, ok).** Rust has no `computed goto`; the closest portable technique is
  a tail-call-dispatch table (one fn per op, `become`/guaranteed-TCO — *currently nightly-only*, a
  std-only/stable tension worth flagging) or a match the compiler can turn into a jump table. Real
  but lower-ceiling than superinstructions and gated on stable TCO; defer behind the cheap wins.
- **I-tco (reject).** Tail-call optimization would let self-recursion run unbounded instead of
  faulting at the 256 MB worker-stack cap — but **PHP has no TCO**, so a recursive Phorj program that
  *succeeds* under TCO would *fail* (stack overflow / fault) under the transpiled PHP, breaking the
  `run ≡ runvm ≡ real PHP` spine for deep-recursion programs. It also changes the observable
  behavior of an existing limit (KNOWN_ISSUES "recursion is depth-limited"). This is a spine-breaking
  optimization, not an invisible one — reject on philosophy + correctness grounds. (Iterative
  rewriting is the PHP-idiomatic answer; the depth cap stays.)

**Net:** original 19 − 0 mis-listed + 6 newly-found = 25 items.
