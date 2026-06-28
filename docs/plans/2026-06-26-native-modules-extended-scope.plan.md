# Native Modules — Extended Scope (Caching / Testing / HTTP-types / Concurrency + more)

> Consolidates the developer's 2026-06-26 follow-up ideas (Caching, native Testing/mocking, rich-typed
> HTTP, threads+async) plus my "what you're missing" additions, into ONE prioritized roadmap, applying
> the same lens as the research SSOT (`docs/research/native-modules/SSOT.md`). Each candidate is being
> **validated one-by-one** with the developer via `AskUserQuestion`; decisions land in the log below.
> Plan location = repo.

## The lens (unchanged from the research SSOT)
**The Determinism Partition** decides feasibility before usefulness. Tier A = pure/deterministic →
byte-identity-gateable, std-only, ships as a `Core.*` module with a gated example. Tier B =
impure/non-deterministic (clock, sockets, threads, fs writes) → quarantined outside the spine
(`Transport`/`pure:false` precedent), fixture-tested, transpiled to PHP. Hard constraints: **zero-dep
std-only Rust (no TLS, no regex, no socket helpers beyond `TcpStream`), the byte-identity spine, and the
PHP 8.5 transpile floor under `php -n`** (mbstring + most ext ABSENT; Fibers/PCRE/hash/BCMath present).

## Candidate challenge verdicts (evidence-graded)

| Candidate | Tier | Feas | Conf | Verdict / reframe |
|---|---|---|---|---|
| **Caching** (persistent/TTL) | B | low (gateable ≈0%) | high | Persistence+TTL+shared-store are all impure (clock; Redis/Memcached need a socket Phorj can't open; APCu absent under `php -n`; fs writes impure). **Defer (Tier-B milestone).** Pure slice = a request-scoped `memoize`/`Cache<K,V>` over the shipped `Core.Map` + higher-order natives — optional small Tier-A helper. |
| **Testing/assert/mock** | A | high | high (assert) / med (auto-mock) | **ADOPT.** Assertions = pure, transpile to plain PHP if-checks + a custom reporter (NOT PHPUnit — Composer pkg, absent under `php -n`). `phg test` runner with deterministic output (no timing/memory, sorted order) = Tier A, top-tier value. Manual mocks via interfaces today; auto-mock via Core.Reflect tables = follow-up slice. |
| **Rich HTTP types** (Response/JsonResponse/StreamResponse/Redirect/…) | A (types) / B (stream+client) | high | high | **ADOPT as an M6 extension.** Response/JsonResponse/RedirectResponse/HtmlResponse = pure subclasses (headers + Core.Json/Core.Html body) over the shipped M6 W1 `Request`/`Response`. StreamResponse *type* pure; *streaming* = Tier B serve runtime (W3). HTTP *client* stays Tier B (no TLS, research-deferred). |
| **Threads** (true parallel) | — | ~0% | high | **REJECT.** `Rc`-shared heap (`Value` not `Send`/`Sync`) forces single-threaded; real threads are non-deterministic → break the spine; no `php -n` target (pthreads/parallel are ext). Fighting the foundation. |
| **Async** (cooperative coroutines) | A/B | medium | medium | **ADOPT-LATER (major M6+ milestone).** PHP 8.1 **Fibers are core** (under `php -n`) → cooperative, single-threaded, *deterministic* scheduler = the "green threads under an unchanged contract." Byte-identity-safe IF scheduling order is deterministic. Real design + effort. |
| **Core.Serde** (typed safe codec) | A | high | med-high | The SAFER `serialize`: no code-exec on decode, decimal/bytes survive, byte-stable. Shares Core.Dump's value-walk + cyclic guard. (Already in the research upgrade-lens.) |
| **Core.Event** (observer/dispatcher) | A | high | medium | In-process typed pub/sub; deterministic if handler order is deterministic (registration order). Pure, decouples architectures. |
| **Core.Cli** (arg/flag parser) | A | high | high | Pure parser over a given arg list (the list comes from Tier-B `Core.Process`). High DX for building Phorj CLIs. |
| **Core.Template** (string templating) | A | high | medium | Pure typed templating; complements Core.Html. (Scope: escaping/interp only; control-flow templating is bigger.) |
| **Core.Uuid** | A (v5/v3) / B (v4) | high | high | Namespace UUIDs (v5/v3 = name + Core.Hash) are deterministic → Tier A now. v4 random = seeded `Core.Random` (deterministic) or Tier B. |
| **Core.Log** | A (record) / B (emit) | high | high | A structured log-RECORD builder is pure (Tier A); actually emitting (timestamp + sink) is Tier B. |

## Recommended order (pending one-by-one validation)
Reconciled with the research SSOT's Tier-A order (Hash→Encoding→Csv→Dump→Validate→Random→Url→Sql→Time)
and these new candidates. Tier A, value÷risk, dependency-driven:

1. **Core.Test (assertions + `phg test` runner)** — highest new value, pure, unblocks disciplined dev of
   every later module. (Manual mocks now; auto-mock follow-up.)
2. **Core.Dump + Core.Serde** (value-walk cluster — build together, single-source the walk + cyclic guard).
3. **Rich HTTP response types** (M6 extension — pure subclasses over the shipped Request/Response).
4. **Core.Cli** (pure arg parser — DX for CLIs).
5. **Core.Uuid (v5/v3 + seeded v4)** — needs Core.Hash/Core.Random.
6. **Core.Event**, **Core.Template**, **Core.Log (record)** — pure, self-contained.
7. **Async/coroutines (Fiber-backed, deterministic scheduler)** — major M6+ milestone.
8. **Caching (persistent)**, **HTTP client**, **DB execution** — Tier-B quarantine milestones (last).

(The research SSOT's Tier-A modules — Hash/Encoding/Csv/Url/Validate/Sql/Time — interleave by dependency;
this list adds the NEW candidates and their relative priority.)

## Round-1 challenge corrections (2026-06-26, developer pushed for fuller scope)

**The reframing correction:** "can't transpile to PHP" was imprecise — *everything here transpiles to
PHP*. The wall is the three-leg **byte-identity** (`run≡runvm≡PHP`), for three escalating reasons:
(1) **non-determinism** (clock/random/network/scheduling); (2) **backend asymmetry** — the *Rust* legs
can't do what PHP can (a persistent cache: PHP hits APCu/Redis, the Rust legs have neither → diverge;
transpiles fine); (3) the one HARD zero-dep wall: **TLS** (no std TLS → Rust legs can't do HTTPS without
a crate; escapes = http-only `TcpStream` or `curl` shell-out). Phorj **already** has the two-tier model
(`Core.Process`/`Core.Env` = `pure:false`, quarantined, fixture-tested). The strategic decision is **how
far to expand Tier B** for high-value impure features (full cache / HTTP client / DB / live concurrency)
— all of which transpile to PHP and are fixture-testable, just not byte-identity-gated.

**Concurrency — one hard NO, three solid YESes:**
- **HARD NO — true parallel threads w/ shared mutable state.** Incoherent on 3 axes: `Rc` heap not
  `Send` (sharing a `Value` across OS threads = compile error; the `Arc`+locks fix taxes the
  single-threaded 99%); non-deterministic (breaks the spine run-vs-run); no `php -n` thread target.
- **YES #1 — cooperative async/await** (deterministic ready-queue → PHP 8.1 **Fibers**, core under
  `php -n`): byte-identical because scheduling is deterministic + single-threaded. Major milestone.
- **YES #2 — pure data-parallelism** (`parallelMap`/fork-join over PURE fns, deterministic merge): the
  API contract is order-deterministic output → all legs sequential today (byte-identical), Rust-side
  physical parallelism is a later optimization that preserves output. Tasks must be side-effect-free.
- **YES #3 — reactive/FRP streams** (library over #1): gated over deterministic sources; Tier B over
  live sources (sockets/timers).

**Faker/mocker:** a **seeded** Faker = deterministic → Tier A (embedded corpora + seeded PRNG, same
constraint as `Core.Random`); unseeded = Tier B. Auto-mocker (synthesize an interface impl recording
calls) = deterministic object-state, feasible via Core.Reflect tables (meatier slice). A full testing
suite (assertions + `phg test` runner + seeded Faker + auto-mocker) is **mostly Tier A**.

## Decisions Log
- [2026-06-26] AGREED: extend the native-modules scope with the developer's 4 ideas + my additions;
  analyze inline (not a re-run workflow), consolidate here, validate one-by-one via `AskUserQuestion`.
- [2026-06-26] DEVELOPER POSITION (round 1, pre-challenge): leaning toward FULLER scope — full
  persistent cache, testing suite incl. Faker + auto-mocker, full HTTP impl, and concurrency/threads/
  reactive as a non-default opt-in; explicitly OPEN to non-byte-identical / non-transpilable features
  ("some things have real value"); wants a hard-no-or-safe-path on concurrency. Challenge delivered
  above. Final per-candidate decisions pending round-2 validation.
- [2026-06-26] AGREED (round 2, post-challenge):
  - **Tier model = CASE-BY-CASE** — no blanket Tier-B charter; impurity decided per feature as it comes
    up (the mechanism still gets designed, but admission is per-feature, not a blanket policy).
  - **Concurrency = all safe paths + a Tier-B live escape** — cooperative async/await (Fiber-backed,
    deterministic, gated) + pure data-parallelism (deterministic merge, gated) + reactive/FRP (gated over
    deterministic sources) **plus** a Tier-B escape for genuinely-live concurrency (real sockets/timers,
    physical side-effecting parallelism — non-gated, fixture-tested). **Shared-state OS threads = HARD
    NO** (incoherent with the `Rc` heap + spine + `php -n`).
  - **Testing = FULL suite** — assertions + `phg test` runner + **seeded** Faker (Tier A) + auto-mocker
    (Reflect-based). Mostly Tier A.
  - **Next step = launch a focused research workflow** on the hard designs (concurrency models +
    deterministic-parallelism/async scheduler + reactive/FRP + the Tier-B mechanism & per-feature
    impurity calls + full HTTP/cache/DB Tier-B designs + the full testing suite) → a design SSOT under
    `docs/research/extended-modules/`.
- [2026-06-26] AGREED (extended-modules SSOT cross-cutting validation, after reviewing
  `docs/research/extended-modules/SSOT.md`):
  - **D-H0 — APPROVE the project-harness fix:** extend `uses_impure_native` to BOTH project harnesses
    (`all_example_projects_*`), gating on the post-load resolved native set (scan every `.phg` under the
    root). This is **Phase-0 prerequisite #1** — it unblocks every Tier-B project walkthrough and reverts
    6 Tier-B "determinism breaks" verdicts to sound. Until it lands: Tier-B examples are single-file flat.
  - **D-PRNG — sub-2^63 shift-add PRNG:** build `Core.Random` so every intermediate stays < 2^63 (Rust
    i64 == PHP signed-int, no float promotion, never `mt_rand`) → seeded random stays three-leg byte-
    identical (Tier A). Pin a vetted <2^62 multiplier + a `mul_mod` Rust-vs-PHP parity fixture as the
    lock gate. **Phase-0 prerequisite #2 (PR0).**
  - **D-G1 — Transport/process-global, REJECT `NativeEval::Effectful`:** impurity lives OUTSIDE the
    `eval` body (set-once-read like `PROCESS_ARGS`; the `serve.rs` `Transport` seam). No cross-surface
    Effects plumbing.
  - **D-Async-1 — ship the suspension-free subset now, defer the suspending core:** `Core.Parallel`
    (map/forkJoin) + reactive A1 are Tier A now; `yield`/`await`/channels wait until a suspension
    primitive is proven on all three legs. **Split live natives into `Core.AsyncLive`/`Core.Time`/
    `Core.Net`** (so the module-granular quarantine fires on the clock import, not the scheduler).
  - **Build order locked:** Phase 0 (H0 + PR0 Core.Random) → Phase 1 Tier A (Parallel → Stream → Faker
    → Memo → HTTP-types → Test → Mock) → Phase 2 Tier B (Cache → Time/Net → HTTP-client → DB),
    reconciled with the native-modules SSOT (its Sql builder gates Db; its Random = PR0). Per-slice
    decisions (D-Stream/D-Test-Q1/D-Test-harness/D-Mock/D-Http/D-Cache/D-Db) resolved as each is built.
