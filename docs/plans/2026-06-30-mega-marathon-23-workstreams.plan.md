# Mega-Marathon — 23 Workstreams (locked 2026-06-30)

> **CLEAN-SESSION ENTRY POINT.** This is the durable backlog for a fully-autonomous
> marathon. The developer chose a generous slate across 6 proposal batches, then asked
> to compact and have a fresh session execute it **top-down, autonomously, until 100%**.
> Read this top to bottom, then start at **A1** and work down. Do NOT re-ask for the
> slate — it is locked below. Only stop for genuine design forks (real ambiguity with no
> defensible default) via `AskUserQuestion`.

## Standing rules (apply to EVERY workstream — no exceptions)

- **Autonomy:** project bypass sentinel
  `~/.claude/projects/-stack-projects-phorj/state/autonomous-3c-bypass` is set → run the
  3C/6C convergence loops **silently at full 30/8 params**. No per-cycle asks, no plan-gate
  asks. Genuine design forks still ask (information gate, not confirmation gate).
- **Correctness spine (the hard gate):** every shipped feature is **byte-identical on
  `run` ≡ `runvm` ≡ real PHP**. Run the full gate before committing:
  `export PATH=/stack/tools/cargo/bin:$PATH` then
  `PHORJ_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php PHORJ_REQUIRE_PHP=1 cargo test --workspace`.
  Transpile floor = **PHP 8.5** (CI also runs an 8.6-dev canary; the bare `php` on PATH is
  8.6-dev and too permissive — always test against the 8.5 floor).
- **Examples ship with features:** every feature lands a runnable `examples/**/*.phg`
  (auto byte-identity-gated by the `tests/differential.rs` glob) + an `examples/README.md`
  entry, in the **same change**. CLI/tooling features get a walkthrough README + a small
  companion `.phg`.
- **Quality gate:** `cargo clippy --all-targets` clean + `cargo fmt --check` clean before
  every commit.
- **Op coupling:** adding an `Op` variant requires extending three exhaustive matches in
  the **same commit**: `src/vm.rs` `exec_op`, `src/chunk.rs` `BytecodeProgram::validate`,
  `src/compiler.rs` `stack_effect`. Prefer **no new Op** (front-end-only / reuse existing
  ops) wherever possible — most language features here can be done front-end-only.
- **CTy operand trap:** if a feature un-rejects an out-of-surface expression whose *result*
  is an arithmetic operand, the VM compiler's `ctype`/`CTy` must learn it too, or `run`
  accepts what `runvm` rejects. Always add an `expr + 1` differential case.
- **Reified-operand threading:** any new vm-compile entry path (playground `runvm`,
  `disasm`, `bench`) must use `check_and_expand_reified` + `compile_with`, not plain
  `compile` — else `run≠runvm` hides off the differential's CLI path.
- **Git:** commit each self-contained green change autonomously (descriptive
  `feat:`/`fix:`/`docs:`/`test:` messages, no `Co-Authored-By`). **Do NOT push** — the dev
  pushes. If the safety classifier blocks a `git commit`, present the exact command for
  manual execution; do not retry.
- **Build the binary after each feature:** `cargo build --release`; the binary is
  `/stack/projects/phorj/target/release/phg`.
- **Repo references:** read `docs/INVARIANTS.md` before touching backends / value kernels /
  the `Op` set; `docs/ARCHITECTURE.md` for the pipeline map. Update `CHANGELOG.md`,
  `KNOWN_ISSUES.md`, `ROADMAP.md`, `docs/MILESTONES.md` as features land.
- **Honesty on milestone-sized items (Phase H):** design-first, scope explicitly, do NOT
  force a multi-session milestone to a fake "100%". Land a real, green, self-contained
  increment + a written design spec, then continue.
- **Status report cadence:** end substantive reports with `GA: ~X% · Global: ~Y%`
  [Speculative]. Last estimate GA ~52% · Global ~42%.

## Priority order (dependency-aware)

Execute phases in order. Within a phase, items are listed in execution order. Easy-win
items (B4 stdlib breadth, E1 perf) may be interleaved as palate-cleansers between harder
items, but do not let them displace the phase order.

---

## PHASE A — Concurrency core (the locked crux + dependents)

### A1. Green-threads cooperative cutover — S4.3 steps 2–5  ✅ **DONE** (`15a756b`)
Cooperative cutover complete + byte-identical end-to-end (`phg run`≡`phg runvm`): both backends host
each green task in a corosensei coroutine driven by the shared `green::sched`; `spawn` defers a
single-overload free-fn call (VM `Op::SpawnCall`, interp deferred task), `recv`/`join` suspend via the
yielder, the flip routes `uses_concurrency` programs (all 8 entry points incl. the project loader).
`spawn consume(ch); send(42)` → `got 42`/`done 42` (eager faulted). 1282 lib / differential 125 / PHP
8.5 / clippy+fmt / wasm. Commits: `7cfa30c` `260292d` `2d4a2c1` `6bd1b0a` `15a756b`. Follow-ups
(KNOWN_ISSUES): method/overloaded/closure-spawn deferral, cooperative fault-trace frames, per-task statics.

<details><summary>original plan</summary>

**Size:** Large. **Status:** infra built+tested (`src/green/{sched,exec,coro}.rs`,
corosensei dep, `ast::uses_concurrency` gate at `27a3381`). **Interpreter half DONE + green
(`7cfa30c`)** — `src/interpreter/coop.rs`: each task runs its own `Interp` in a corosensei coroutine
driven by the shared scheduler; `spawn` defers (free-fn body as the coroutine root, no lambda);
`recv`/`join` suspend via the yielder; `Interp` gained lifetime `<'c>` for the optional `&dyn Suspend`
(closure-local, no-unsafe deep-suspend per `green::spike`). Gated OFF (`#[allow(dead_code)]`) — the flip
needs BOTH backends. **Found:** the `Vm<'a>→Rc` refactor is NOT needed (the `'static` closure captures
the `Rc<program>` and builds the engine inside). **REMAINS = VM half + flip** — full pickup design in
memory [[marathon-a1-interp-coop-engine]]: VM cooperative driver (run a fn via `run_until` capturing the
value), VM recv/join suspend, the **hard part = VM spawn-defer co-design** (compiler emits a
function-index spawn, NOT a lambda — the reverted `b5053a4` trace bug), then the same-commit flip of
`cmd_run`/`cmd_runvm` + differential litmus.

**The crux:** thread the borrowed coroutine yielder into the recursive interpreter
**without a lifetime on `Interp`** and **without `unsafe`** (crate is
`#![forbid(unsafe_code)]`).

**Steps:**
1. *(done, `27a3381`)* `ast::uses_concurrency` gate detector.
2. Host each task in a corosensei coroutine on **both** backends — interpreter task is
   `'static`-movable; VM must hold `Rc<BytecodeProgram>` (not `&'a`).
3. `spawn` **defers**: eval args eagerly, push a `CoroutineTask` whose root is the
   **function call itself** (NOT a synthetic lambda wrapper — that was the reverted
   thunk-trace bug at `b5053a4`).
4. `recv`/`join`/`yield` suspend via `CoopCtx.suspend`; `send` → `on_send`; drive via
   `green::exec::run_loop`. Build as tested `run_cooperative_{interp,vm}` fns.
5. Tiny entry-point flip (both backends, **same commit** — the spine never goes red).
   **wasm keeps eager** (`#[cfg(target_arch="wasm32")]` — stackful coroutines fail wasm32).

**Acceptance:** litmus `spawn consume(ch); send(42)` yields `got 42` / `done 42`
byte-identical `run ≡ runvm`; `examples/concurrency*.phg` green on both backends + (where
deterministic) PHP. Plan detail: the existing
`2026-06-29-big-marathon-...-concurrency.plan.md` → "S4.3 COOPERATIVE CUTOVER" section.

</details>

### A2. Generator `yield` + lazy sequences
Lazy generators that pair naturally with the A1 coroutine engine — `function gen() yields int { yield 1; yield 2; }`,
consumed by `for-in` (needs B1). Reuses the suspend/resume machinery from A1. Transpiles to
PHP `Generator`/`yield`. **Do after A1** (shares the engine).

### A3. async/await + structured concurrency sugar
On top of A1: `async fn` / `await`, `Task.all([...])`, `select`/race over channels,
timeouts. Ergonomic layer over green threads. **Do after A1.**

---

## PHASE B — Iteration & collections foundation

### B1. Iteration protocol: `for-in` over Map/Set/String + `enumerate`/`zip`
`for (k, v in map)`, `for (x in set)`, `for (ch in str)`, `for (i, x in enumerate(xs))`,
`zip(a, b)`. Foreshadowed by the R1 insertion-ordered Map/Set rep. Foundation for B2, A2,
D1. Front-end + a couple natives; transpiles to PHP `foreach`. **Do early** — many things
depend on it.

### B2. Comprehensions
`[x*2 for x in xs if x > 0]` list comprehensions + `[k => v for ...]` map comprehensions.
Depends on B1's iteration. Lowers to a loop+accumulate in the parser/checker (front-end;
prefer no new Op). Part of the "ergonomics pack" + the generator pick.

### B3. Tuples + multiple return values
`(int, string)` tuple type, tuple literals `(a, b)`, destructuring `var (a, b) = pair`,
multiple return. A genuinely new type — reuses List/Map value plumbing. Transpiles to PHP
`[a, b]` + `list()`/array destructuring. Medium.

### B4. Stdlib breadth blitz  *(easy wins — interleavable)*
Large batch of `Core.List`/`Core.Map`/`Core.Set`/`Core.Text`/`Core.Math` additions, each
byte-identical + guide example + PHP-oracle. Use the documented **collection-native recipe**
and watch PHP numeric-string parity gotchas. Low risk; good palate-cleanser between hard items.

---

## PHASE C — Language polish (mostly independent front-end)

### C1. Enum methods + associated functions
`enum Color { Red, Green; fn hex() -> string { match this { ... } } }` + static associated
fns. Closes the deferred "generic enum methods" gap. Reuses method machinery; front-end.

### C2. Match extras: or-patterns, range patterns, `@` bindings
`A | B =>` or-patterns, `1..5 =>` range patterns, `n @ 1..10 =>` binding patterns. Rounds
out the pattern cluster. Front-end-only; reuse existing match ops.

### C3. String formatting: `Core.Fmt` + interpolation format specs
printf-style + format specifiers inside interpolation: `"{pi:0.2f}"`, padding/alignment/
width. Lexer (interpolation grammar) + natives. Byte-identical to PHP `sprintf`. High
day-to-day value.

### C4. Language ergonomics pack  *(residual sugar — scope at start)*
Whatever ergonomic sugar is NOT already covered by B2/C2/C3 — candidates: richer string
methods, chained optionals, terser lambdas, etc. **Scope the exact set at the start of this
item** (read ROADMAP.md + KNOWN_ISSUES.md for deferred ergonomics) and pick the
highest-value front-end-only wins.

### C5. core.json + dynamic `Any`/`Json` type
Unlock the long-deferred `Core.Json` module by adding a dynamic `Json`/`Any` type (`Ty`
currently has no type variable for it). Use the **injected-type pattern**
(`cli::inject_*_prelude` before check, gated on import) + reserved enum-variant mangling, as
documented in the `core-json-and-injected-types` memory. Medium language work + a json guide
example with round-trip.

---

## PHASE D — Protocols

### D1. Protocols/traits: Comparable / Equatable / Iterable / Display + operator dispatch
Standard protocols a type can satisfy, with operator/method dispatch (`<`, `==`, `for-in`,
string conversion) routing through them. Ties iteration (B1) + traits + match together.
Depends on B1. High leverage; design-first.

---

## PHASE E — Performance (bench-gated)

### E1. M-perf VM optimization pass  *(interleavable)*
Slot-indexed fields Phase B, dispatch/hot-path tuning, broader inline-cache coverage. Every
change gated by `phg bench <file>` before/after numbers + output-identity. No surface change.

### E2. Incremental / cached compilation
Cache compiled units keyed by content hash so re-runs/re-builds skip unchanged packages.
Reuses the FNV-1a-64 content-hash machinery from `bundle`/`vendor`. Bench-gated.

---

## PHASE F — Tooling / DX

### F1. `phg repl` — interactive shell
Read-eval-print loop with persistent session bindings. Reuses the existing eval/check
pipeline. Tooling (not on the byte-identity spine) — still gets tests + a walkthrough README.

### F2. `phg doc` — documentation generator
Generate browsable docs from doc-comments + signatures (rustdoc-style), output
HTML/Markdown. Grows the public surface; feeds G2.

### F3. Debugger: step-through debugging (DAP)
Debug Adapter Protocol server — breakpoints, step in/over/out, inspect locals/stack — wired
into the VM. Editor-integrable like the existing LSP. Tooling milestone.

---

## PHASE G — Showcase & public surface

### G1. Showcase: a real Phorj application + tutorial-grade examples
One complete real program end-to-end in Phorj (a small CLI tool or a web service over the M6
handler model) + a guided "chapter" of examples. Stress-tests the whole language; grows the
public surface. **Do after enough features exist** (late).

### G2. Online docs site + WASM playground polish
Public-facing docs/landing site (consuming `phg doc` output, depends F2) + a polished
in-browser playground (examples, share links, error rendering). Showcase + adoption.

---

## PHASE H — Milestone-sized (design-first; scope honestly, may span sessions)

### H1. True multicore parallelism (Send-safe actor model)  *(MILESTONE)*
The honest "real threads" answer. Blocked by the `Rc`-shared heap (`Value` isn't `Send`).
Path: a **message-passing actor model** — per-worker isolated heaps + Send-safe channels —
since shared mutable state across cores would need an `Arc` heap rewrite. `phg serve` already
does real OS-thread parallelism at the *request* boundary (`Arc<Program>`, S4.1/4.2).
**Design-first; land a real green increment + spec, do not force a one-session 100%.**

### H2. Compile-time metaprogramming / macros  *(MILESTONE)*
Hygienic compile-time code generation (derive-style or template macros), **expanded out
before backends** — same "erase before backends" discipline as type sugar / `erase_generics`.
Large; design-first.

### H3. FFI / native extension interface  *(MILESTONE)*
A typed boundary to call native/PHP capabilities beyond pure transpile (`declare foreign`
already exists for the PHP target — M8.5). Extend toward a real extension ABI. Target-aware
(must not break the byte-identity spine); design-first.

### H4. Editions / versioning (M13)  *(MILESTONE)*
Opt-in language editions with backward compat (`edition = "2026"` in `phorj.toml`), so
breaking changes land without breaking old code. The post-1.0 evolution mechanism.
**Config must be compile-time** (all backends), never runtime. Design-first.

---

## Decisions Log
- [2026-06-30] AGREED: developer selected ALL workstreams across 6 proposal batches (23
  total) for a fully-autonomous marathon; commit green, do NOT push.
- [2026-06-30] AGREED: priority order = green-threads cutover (A1) FIRST, then work
  top-down through phases A→H; milestone-sized items (H1–H4) are design-first and scoped
  honestly, not force-finished.
- [2026-06-30] AGREED: concurrency layering clarified — cooperative coroutines/fibers
  (A1) now; async/await sugar (A3) next; true shared-memory multicore (H1) is a future
  milestone gated by the `Value: !Send` heap constraint.
- [2026-06-30] AGREED: developer will `/compact`, then a clean session executes this plan
  autonomously to 100%.

## Formal Plan
The phase/item breakdown above IS the formal plan. Acceptance criteria per item; standing
rules (top) are the gate every item passes before commit. Active-plan pointer set to this
file.
