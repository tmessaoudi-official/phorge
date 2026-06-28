# Native Modules — Research & Build Initiative (Plan + Decisions Log)

> Durable SSOT for the "rich native modules" initiative (HTTP/URL, var-dumper, DB, + more) raised
> 2026-06-26. Survives compaction. Plan location = repo. Status: **research scoped + ordered; deep
> multi-agent research workflow APPROVED (Option 1), to be launched in the next session.**

## How to resume after compaction
The developer does **NOT** need to re-paste the original prompt. Next session: read this file, then
launch the approved multi-agent research workflow (§Workflow below). One-line trigger is enough, e.g.
*"run the native-modules research workflow"*. M-NUM S4 (the active milestone's last slice) is still open
and may be finished first if the developer prefers (see order item 0).

## Decisions Log
- [2026-06-26] AGREED: pursue **ALL** proposed native modules (not a subset) on a **recommended order**
  (below), value÷risk + dependency driven.
- [2026-06-26] AGREED: research depth = **Option 1, a multi-agent research workflow** (real token cost):
  prior-art sweep (PHP/Python/Go/Rust stdlibs), per-module feasibility spike, dumper-format + SQL-builder
  designs, adversarial byte-identity review → a written research SSOT. Launch **after compaction**.
- [2026-06-26] AGREED (new scope): also analyze **"what PHP can do today that Phorj cannot, and how to
  port each to PHP in a *better* way"** — not just parity, but the provably-correct/typed UPGRADE lens
  (Phorj:PHP :: TS:JS). Extends the existing parity SSOT `docs/specs/2026-06-21-php-parity-and-beyond.md`
  (which covered *language* gaps); this new pass focuses on **stdlib/library capability** gaps + the
  "better-than-PHP port" angle.
- [2026-06-26] LOCKED FRAMING — **the Determinism Partition** decides feasibility before usefulness:
  every capability is **Tier A (pure/deterministic → byte-identity-gateable, std-only, ships like any
  Core.* module)** or **Tier B (impure/non-deterministic → quarantined outside the spine via the M6
  `Transport` model, fixture-tested, transpile-to-PHP)**. Driven by the three constraints: byte-identity
  spine, determinism, **zero dependency (std-only; NO TLS, NO regex, NO http/serde crates — `[dependencies]`
  is empty, verified)**.

- [2026-06-26] AGREED (sequencing): developer chose **research workflow FIRST, then M-NUM S4** (overriding
  the plan's optional "S4 first" item 0). Launch the 5-stage multi-agent research workflow now → SSOT under
  `docs/research/native-modules/`, then build + commit M-NUM S4 to close M-NUM.

## Feasibility verdicts (challenged, with confidence)
- **`Core.Dump` (var-dumper, symfony-like): ~100% feasible, Tier A.** Reuses the existing cyclic
  visited-set in `value::eq_val_rec` for circular refs; exhaustive over the closed `Value` enum; depth/
  string limits/colors all deterministic. **LAW that makes it 100%:** define a *Phorj* deterministic
  format (NO addresses/object-ids/resource handles) and emit an identical `__phorj_dump()` PHP helper —
  NOT PHP's native `var_dump` (its `#1` ids are non-deterministic → byte-identity impossible).
- **`Core.Url` (parse/build/query-encode): ~100% feasible, Tier A.** Maps to `parse_url`/`http_build_query`/
  `urlencode`. The feasible half of "Guzzle"; pairs with M6 W1's `Request`/`Response`.
- **HTTP client (Guzzle): NOT pure-native (~0% as Tier A).** Rust std has **no TLS** → native HTTPS is
  infeasible without a crate (breaks zero-dep), a `curl` shell-out (impure/platform), or hand-rolled
  crypto (rejected). Feasible ONLY as **Tier B**: value types (already have Request/Response) + a
  `Transport` (curl-shell or `http://`-only TcpStream; transpile→PHP curl) + local-fixture-server tests
  outside the spine. HTTPS forces the curl-shell-vs-http-only decision.
- **Database: NOT pure-native (~0% as Tier A).** Connections are the most non-deterministic surface.
  Tier B: `Connection`/`Statement`/`Row` value types + driver (most tractable: **SQLite via FFI**, or use
  the **`/stack` docker Postgres/MySQL** as integration fixtures) + transpile→**PDO** + fixture tests
  outside the spine. The **pure, gateable half = a typed `Sql` query builder** (escaping/binding →
  injection-safe by construction) — ship that as Tier A independently.
- **Regex / PCRE: big honest gap.** Rust std has no regex engine; byte-for-byte PCRE parity is a
  milestone-sized, correctness-brutal build. Options: hand-roll a *documented subset*, or accept as a
  permanent gap. Low confidence on full parity.
- Don't hand-roll crypto/TLS or secure RNG; gzip header carries a non-deterministic timestamp.

## Recommended build order (all of them) + WHY
**0. (optional) finish M-NUM S4** — `Core.Math` breadth + `number_format`; closes the active milestone
   before opening this front. *Why first: don't leave a milestone half-open.*
**Tier A (pure, gateable — build first; lower risk, establishes the modular pattern):**
1. **`Core.Dump`** — *highest value ÷ risk*; pure, ~100%, daily DX, reuses cycle-detection. *Why #1:
   biggest payoff, smallest risk, no deps on other new modules.*
2. **`Core.Url`** — pure, ubiquitous, pairs with M6 server. *Why #2: high value, self-contained.*
3. **`Core.Encoding`** (base64/hex/url-encode) — trivial std-only, **foundational** (Hash/Url/HTTP need
   it). *Why here: a dependency of #4 and the Tier-B work.*
4. **`Core.Hash`** (crc32/md5/sha1/sha256) — pure ~200-line hand-rolls; cache keys/integrity. *Why after
   Encoding: needs hex output.*
5. **`Core.Random` (seeded)** — *seeded → deterministic → gateable*; foundational for UUID/testing.
   *Why here: unlocks deterministic UUID-v5/test fixtures.*
6. **`Sql` builder** — typed, injection-safe query builder + escaping. *Why before DB exec: it's the pure
   half and the API the Tier-B driver will consume.*
7. **`Core.Validate`** (`filter_var`-like, returning `T?`) — pairs with the type system; *better than
   PHP's loose filter_var*. *Why after Url: url/email validation reuses Url.*
8. **`Core.Csv`** — pure, self-contained data chore.
9. **M-TIME** (date arithmetic + format/parse pure; `now()` is Tier B) — already a planned milestone; pure
   parts gateable. *Why here: large, partly-planned; pure parts slot into Tier A.*
**Tier B (impure, quarantined — build last; establishes the Transport pattern, lives outside the spine):**
10. **HTTP client** — first Tier-B capability; mirrors the M6 server (Request/Response already exist).
    *Why first in B: cleanest impure model, sets the Transport pattern DB will reuse.*
11. **DB execution** — consumes the #6 Sql builder + the #10 Transport pattern; transpile→PDO. *Why after
    HTTP: reuses the quarantine machinery.*
12. **Regex** — biggest unknown; subset-or-defer; likely its own milestone. *Why last: highest risk/effort.*

**Order rationale (themes):** (a) Tier A before Tier B — gateable wins first, build the modular native
pattern, defer the spine-breaking work; (b) dependency order (Encoding→Hash, Url→Validate, Sql→DB,
HTTP-pattern→DB-pattern); (c) value÷risk descending within each tier; (d) Regex last (biggest unknown).

## Workflow (Option 1 — launch next session)
A research-phase fan-out (pipeline by default), producing a written SSOT under `docs/research/native-modules/`:
- **Stage 1 — prior-art sweep** (parallel, one agent per ecosystem lens): PHP stdlib + ext (Guzzle, PDO,
  symfony/var-dumper, Carbon, PCRE), Python stdlib, Go stdlib, Rust std/ecosystem — *what each provides,
  what's pure vs impure, what API shape*.
- **Stage 2 — per-module feasibility spike** (pipeline per candidate module): std-only feasibility, byte-
  identity strategy, transpile target, Tier A/B classification, confidence.
- **Stage 3 — PHP-can / Phorj-can't + better-port** (new scope): enumerate stdlib capabilities PHP has
  that Phorj lacks; for each, the *better* port (typed/deterministic/safer), referencing the language
  parity SSOT to avoid overlap.
- **Stage 4 — designs**: `Core.Dump` deterministic format + `Sql` builder shape (the two highest-value).
- **Stage 5 — adversarial byte-identity + completeness critic**: refute each "pure" claim; flag any
  non-determinism (ids, ordering, clock, addresses) that would break the spine.
- Synthesis → one SSOT research doc + a per-module adopt/defer table feeding individual build plans.

## Craftsmanship guardrails (developer mandate)
One `src/native/<leaf>.rs` per module, each native a small `NativeFn` keyed by `(module,name)`; no god-
file. **Audit flag:** `text.rs` (19K)/`mod.rs` (18K)/`list.rs` (17K)/`json.rs` (15K) are heavyweights —
verify no single function is a whale and `mod.rs` (coordinator) isn't accreting logic; fold a targeted
decomposition audit into the next module built. SOLID + TDD + byte-identity-gated example per module
(the "examples ship with features" rule).
