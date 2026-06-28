# Stage 2b — Adversarial byte-identity refutation: `Core.Db` (PDO-like DB execution)

**Charge:** refute the claim that `Core.Db` can stay byte-identical across `run` / `runvm` /
real-PHP-8.5. Default to `determinism_holds=false` on any real risk.

**Outcome:** the spike's own verdict already states determinism does NOT hold (Tier B, 25%). The
adversarial review *confirms and strengthens* that conclusion — byte-identity is not merely unlikely,
it is **impossible by construction, and for a stronger reason than the spike's primary argument**.
The spike's load-bearing source claims were independently re-verified and all hold. I found no path by
which this becomes Tier A. I did find that the spike, if anything, *understates* the divergence even on
its proposed PHP-leg-only fixture test.

`determinism_holds = false.` Verdict: **REFUTED-CONFIRMED** — the byte-identity claim cannot hold;
spike's tier B / defer stands, with one correction to its framing (below).

---

## Source claims re-verified (independent of the spike's prose)

All four anchors the spike rests on are real:

- **Empty deps + `#![forbid(unsafe_code)]`** — `Cargo.toml` has no `[dependencies]` table for the core
  crate; `src/lib.rs:3` and `src/main.rs:3` both carry `#![forbid(unsafe_code)]`. wasm-bindgen is
  confined to the `playground/` workspace member (wasm32-only). [Verified: read Cargo.toml + grep].
  ⇒ no DB driver crate, no FFI to libpq/libsqlite3 (FFI is `unsafe`), no hand-rolled crypto. The Rust
  legs genuinely cannot open a connection.
- **`pure` flag drives the skip, not hardcoding** — `src/native/mod.rs:62` declares `pub pure: bool`;
  `tests/differential.rs:916-924` builds the impure-module set by `registry().filter(|n| !n.pure)` and
  applies it at lines 1004 and 1904. [Verified: read both]. ⇒ marking `Core.Db` `pure:false` does
  auto-quarantine importing programs with zero harness edits.
- **`Value` is a closed enum with NO resource variant** — `src/value.rs:14`: Int/Float/Decimal/Bool/
  Str/Bytes/Unit/Null/List/Map/Set/Instance/Enum/Closure. No `Resource`. [Verified: read]. ⇒ a live
  connection handle does not fit; the spike's "needs a Value change or a magic Instance hack" is real.
- **PDO core-available under `php -n`** — the spike claims `class_exists("PDO")` etc. return true on the
  8.5.7 floor build. I did not re-run the live probe (single-leg detail; not load-bearing for the
  refutation), so I grade this [Unverified — relying on spike's pasted var_dump]. It does NOT change
  the verdict either way: even if PDO is present, the Rust legs still can't match it.

---

## Refutation 1 — byte-identity fails for a *stronger* reason than non-determinism (the decisive one)

The spike's headline argument is "the result is non-deterministic (live state, row order, clocks), so
no golden exists." True, but **even a perfectly deterministic, frozen, single-row, `ORDER BY`-pinned
fixture DB would still break byte-identity** — because two of the three backends produce **nothing at
all**. `run` and `runvm` cannot connect (no std DB API, no driver, no FFI), so they must **stub-error**
(`"Db unavailable on the interpreter/VM"`), while the PHP leg returns rows. The three outputs differ by
*every* byte, deterministically. Byte-identity requires three identical stdout streams; here you have
{error, error, rows}. This is not a flaky-golden problem — it is a **missing-backend** problem.

This is categorically weaker than the `Core.Process`/`Core.Env` precedent the spike invokes: there all
three legs *execute and produce a real result* (the skip exists only because env/argv need not match
across processes). `Core.Db`'s Rust legs can't execute at all. So the quarantine *mechanism* fits (the
`pure:false` skip removes it from the gate), but what's being quarantined is a **PHP-only feature with
dead Rust legs**, not a tri-backend feature merely held off the gate. [Verified: §2 of the spike +
the empty-deps/forbid-unsafe facts above]. This is the correct framing and the spike states it in §4/§8
— I am elevating it from a secondary note to *the* reason determinism fails.

## Refutation 2 — the PHP-leg-only fixture test the spike proposes is itself MORE fragile than stated

Even granting "test only the PHP leg in `tests/db.rs` against `/stack` docker fixtures with mandatory
`ORDER BY` and exact goldens," several divergence traps remain that the spike lists incompletely:

- **PDO `FETCH_ASSOC` value typing is engine/driver-dependent.** `pdo_pgsql` and `pdo_mysql` return
  *all* column values as PHP **strings by default** (unless `PDO::ATTR_STRINGIFY_FETCHES`/native-types
  emulation differs), but `pdo_sqlite` can return native ints/floats depending on `PDO::ATTR_*` and the
  declared column affinity. So a golden for `SELECT id ...` is `"7"` on one driver and `7` on another —
  the spike's "type every column `string?`" mitigation only holds if the test pins
  `PDO::ATTR_STRINGIFY_FETCHES=true` and asserts string goldens; otherwise the float-echo Ryū trap the
  spike flags (risk 5) DOES cross the boundary. [Inferred: PDO driver behaviour is well documented; not
  re-run here]. ⇒ the fixture golden is per-driver, not a single golden — more fragile than "exact
  golden" implies.
- **`NULL` rendering.** A SQL `NULL` → PHP `null`; if the test echoes a row map naively (`echo $v`),
  `null` prints as empty string while a Phorj `Null` value may render differently — but since the Rust
  legs don't run, this only bites the PHP-vs-expected-golden, not cross-backend. Still: the golden must
  encode the null-rendering convention explicitly. [Inferred].
- **`php -n` connection requires the driver's *runtime* deps.** `class_exists("PDO")` being true is
  necessary but not sufficient: `pdo_pgsql` needs libpq present in the process; the fixture test runs a
  real container connection, so this is an integration concern, not a transpile-floor concern — but it
  means `tests/db.rs` is a network/integration test, not a hermetic unit test, and is non-deterministic
  on timing/availability (it can flake on container-not-ready). [Inferred: standard PDO behaviour].

None of these resurrect byte-identity (the Rust legs still can't run); they show the **fallback** test
strategy is weaker / more conditional than the spike's confident "exact goldens" wording.

## Refutation 3 — no Tier-A escape hatch survives scrutiny

I checked whether any sub-surface of `Core.Db` could be carved into Tier A (the way `Core.Sql` builder
is). It cannot:
- The **query builder** half is already correctly separated as the pure `Core.Sql` (Tier A) — that's
  not `Core.Db`. `Core.Db` is, by definition, the *execution* half.
- A **mock/in-memory deterministic DB** in pure Rust to make `run`/`runvm` produce gate-able output
  would (a) require a hand-rolled SQL engine (absurd scope, rejected by the spike, agreed), and (b) by
  definition NOT match real Postgres/MySQL/SQLite semantics — so the PHP leg (real PDO) would still
  diverge from the Rust mock. Self-defeating. [Verified: logical necessity].
- `sqlite::memory:` is sometimes floated as "deterministic" — but it still needs `pdo_sqlite` on the
  PHP side and a Rust sqlite client (forbidden dep) on the Rust side. Same wall. [Verified: empty-deps].

## Floats / locale / ordering — confirming the named risks are real

- **Float round-trip (Ryū vs PHP 14-digit echo)** — a real, documented KNOWN_ISSUE in this repo; if any
  numeric column escapes as a float it diverges. The `string?` typing mitigates only if stringify is
  forced (Refutation 2). [Verified: matches the repo's standing float-format known-issue].
- **Row ordering** — Postgres heap order ≠ MySQL clustered-index order ≠ SQLite rowid; mandatory
  `ORDER BY` is the only fix, and even then a tie without a unique sort key is engine-defined. [Verified:
  standard SQL semantics]. Confirms spike risk 2.
- **HashMap ordering** — N/A here in a damaging way: Phorj's `Value::Map` is insertion-ordered
  (`Rc<Vec<(HKey,Value)>>`, not a `HashMap`), so the *Phorj* map preserves PDO `FETCH_ASSOC` column
  order — good — but column order is still SQL-`SELECT`-list order on the PHP side, which is fine as
  long as the SELECT pins columns. Not a divergence *given* explicit column lists. [Verified: value.rs
  Map is a Vec of pairs].
- **Clocks / `lastInsertId` / sequences** — counters and clocks, non-reproducible. [Verified: SQL
  semantics]. Confirms spike risks 3,4.

## One correction to the spike's framing (not a verdict change)

The spike grades confidence **high** and that is justified for the *infeasibility* conclusion. But its
abstract for downstream readers should lead with **"two backends cannot execute it"** (missing-backend)
rather than **"the result is non-deterministic"** (flaky-golden) — the former is the stronger, simpler,
unconditional argument and is immune to the "but what if you freeze the DB?" rebuttal. The
non-determinism points are all true but are the *second* reason, not the first.

---

## Verdict

- `determinism_holds = false` — byte-identity across run/runvm/real-PHP-8.5 is impossible by
  construction (missing Rust backends + non-deterministic result). [Verified].
- Tier remains **B**. Feasibility for *byte-identity-spine inclusion* is **0%**, not 25% — there is no
  spine path at all. The 25% in the spike conflates "feasibility of shipping the feature at some future
  milestone via PHP-only" (defensible) with "feasibility of byte-identity" (zero). For the byte-identity
  question this review owns, the honest number is **~0–5%** (the 5% only as epistemic humility on the
  unverified PDO-present probe).
- The defer/adopt-later recommendation stands; the `Core.Sql` builder (Tier A) is the gateable sibling
  to ship first.
- Refutations: missing-backend (decisive), per-driver PDO typing fragility on the fallback test, no
  Tier-A carve-out survives, plus the confirmed float/ordering/clock risks.

**Confidence: high** — all load-bearing source facts re-verified in-tree; the one unverified item
(live PDO-present probe) cannot move the verdict.
