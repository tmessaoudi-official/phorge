# Feasibility Spike — `Core.Db` (database execution, PDO-like)

**Module:** Database execution / connection (`Core.Db`)
**Starting hypothesis:** Tier B; most non-deterministic.
**Verdict:** **Tier B — adopt-later (defer to M6/M-Batteries DB slice).** Not rejectable (it is the
inevitable runtime counterpart of the pure `Core.Sql` builder), but it cannot ship now, before
`Core.Sql` exists and before the `Transport`/quarantine I/O story is settled for a stateful resource
(an open connection handle, not a one-shot read).

---

## 1. The determinism partition decides this before usefulness

`Core.Db` is the textbook **Tier B** module. Every facet is non-deterministic w.r.t. the program text:

- **Connection** depends on a live server, credentials, network — none reproducible in a byte-identity
  gate.
- **Query results** depend on the *current database state*, which evolves outside the program. The same
  `SELECT` returns different rows on two runs; there is no fixed golden.
- **Row/column ordering** without an explicit `ORDER BY` is engine- and storage-defined (Postgres heap
  order, MySQL clustered-index order, SQLite rowid order) — three different orders for "the same" query.
- **Auto-increment IDs, timestamps, `NOW()`/`CURRENT_TIMESTAMP` defaults, sequence values** are all
  clocks/counters — the exact traps the prior-art digest names (clock, OS-iteration-order).
- **Floats** coming back from the DB re-trip the known `__phorj_str`/Ryū divergence (14-digit PHP echo
  vs Rust formatting) on top of everything else.

This places it firmly with `Core.Http`, `Time.now()`, true-RNG, and filesystem **writes**. It is the
runtime sibling of the **pure** `Core.Sql` builder (Tier A — escaping/binding/string assembly, byte-
identity-gateable): the spec's intended pairing is `Sql.select(...).build()` → `(sql, params)` fed into
`Db.connect(dsn).query(sql, params)`. The builder is the gateable half; `Db` is the quarantined half.

---

## 2. std-only feasibility (the Rust legs)

**This is the hard blocker.** Phorj's `[dependencies]` is empty and `#![forbid(unsafe_code)]` is on the
crate roots (verified: `Cargo.toml` — only `wasm-bindgen` exists, confined to the wasm32-only
`playground/` workspace member). The interpreter and VM are pure-Rust, zero-dep.

To execute a query on the **Rust** legs (`run`/`runvm`) you need an actual database client:

- **A real DB driver crate** (`rusqlite`, `postgres`, `mysql`, `sqlx`) — **rejected by the zero-dep
  invariant.** Every one pulls a transitive tree (often C bindings → `unsafe`, breaking
  `forbid(unsafe_code)`).
- **SQLite via FFI** (linking `libsqlite3` and writing the bindings by hand) — **rejected:** requires
  `unsafe` (FFI is inherently `unsafe`), which is `#![forbid]`-ed crate-wide. Even waiving that, hand-
  writing a sqlite3 FFI surface is a large, error-prone effort with no determinism payoff.
- **A hand-rolled SQL engine in pure Rust** — absurd in scope (a from-scratch storage engine + query
  planner); rejected on effort alone, and still wouldn't match Postgres/MySQL semantics.
- **Speaking a wire protocol over `std::net::TcpStream`** (the Postgres/MySQL frontend protocols are
  documented and TCP-only — no TLS needed for a local trusted socket, and `std::net` *is* available) —
  **technically std-only-possible** but: (a) it's a multi-hundred-line protocol implementation per
  engine (startup/auth/extended-query/row-description parsing), (b) it is pure I/O → cannot be on the
  byte-identity spine anyway, (c) MySQL's default auth (`caching_sha2_password`) needs RSA/SHA crypto in
  the handshake, which collides with the "crypto/TLS must NOT be hand-rolled" policy. Postgres with
  `trust`/`md5` auth on a local socket is the only semi-tractable target, and even that is a real
  protocol project, not a native.

**Conclusion:** there is **no acceptable std-only way to actually connect & query from the Rust
backends.** This is categorically different from `Core.File.read` (a single `std::fs` call) or
`Core.Process`/`Core.Env` (a single `std::env` read). Those impure natives still *execute* on the Rust
legs trivially because the std API exists. `Core.Db` has **no std API**, so the Rust legs literally
cannot run it without a forbidden dependency.

This means the `Core.Process`/`Core.Env` quarantine model (which keeps a fully-working Rust
implementation, just off the differential) **does not transfer cleanly**: there, `run`/`runvm` still
produce a real result. For `Core.Db`, the Rust legs would have to **stub** (e.g. error "Db unavailable
on the interpreter/VM; transpile to PHP to use it") — a strictly weaker story.

---

## 3. The transpile target (the PHP leg) — this part is easy and core-available

**Verified live on the 8.5 floor build** (`/stack/tools/phpbrew/php/php-8.5.7/bin/php -n`):

```
class_exists("PDO")          => true
extension_loaded("pdo_sqlite") => true
extension_loaded("pdo_mysql")  => true
extension_loaded("pdo_pgsql")  => true
```

So under the oracle's `php -n`, **PDO and the sqlite/mysql/pgsql drivers are all compiled in** — the
transpile target is genuinely available (this is a real, checkable finding, not an assumption; it
defeats the usual `php -n` "extension absent" trap that bites mbstring/intl/curl). PHP transpilation is
the *natural and idiomatic* path:

```php
$conn = new PDO($dsn, $user, $pass);                 // Db.connect
$stmt = $conn->prepare($sql);                        // Db.query (prepared)
$stmt->execute($params);                             // bound params (from Core.Sql builder)
$rows = $stmt->fetchAll(PDO::FETCH_ASSOC);           // rows as List<Map<string,?>>
```

A native's `php:` closure can emit exactly this. **The transpiler half is small and clean.** The
problem is never the PHP leg — it's that the Rust legs can't match it and the result isn't gateable.

---

## 4. Byte-identity strategy — there is none (by construction)

`Core.Db` **cannot be on the byte-identity spine.** Even setting aside §2 (the Rust legs can't connect),
the *result* is non-deterministic: it depends on live DB state, row order, clocks, sequences. The
`uses_impure_native` skip in `tests/differential.rs` (derived from `NativeFn::pure == false`, no
hardcoding) **already** auto-excludes any program importing an impure module — so marking `Core.Db`
`pure: false` quarantines it from the differential with **zero harness edits** (the seam was built for
exactly this; verified in `src/native/mod.rs:62` + `tests/differential.rs:916`).

How it would actually be tested (the `Core.Process` precedent, extended):
- **`tests/db.rs`** (outside `differential.rs`), against a **fixture database** — and here the prior-art
  hint about *"/stack docker Postgres/MySQL as fixtures"* is the right call: `/stack` ships
  `01postgres18`/`01mysql9` containers (host ports in the 42700–42899 range). A test seeds a known
  schema + rows, runs a fixed `SELECT ... ORDER BY`, and asserts an exact golden. **The `ORDER BY` is
  mandatory** — without it the row order is engine-defined and the golden is flaky.
- But this tests **only the PHP leg** (PDO), because the Rust legs can't connect (§2). So even the
  fixture test is single-leg, unlike `tests/process.rs` which tests a real Rust implementation.

**Net:** the quarantine *mechanism* fits, but the thing being quarantined is weaker than Process/Env —
it's a PHP-only feature with stub Rust legs, not a real tri-backend feature merely kept off the gate.

---

## 5. Phorj API sketch

```phorj
import Core.Db;
import Core.Sql;   // the pure builder — its output feeds Db

// Connection is a stateful, impure resource handle.
Db.Connection? conn = Db.connect("sqlite::memory:");      // -> Connection? (null on failure, or throws E)
// or with creds:
Db.Connection? conn = Db.connect("pgsql:host=localhost;dbname=app", "developer", "developer");

// Pair with the pure Sql builder (Tier A, gateable):
{ string sql, List<Value> params } q = Sql.select(["id", "name"]).from("users").where("age > ?", [18]).build();

// Query: rows as a List of Maps (string column -> nullable value).
List<Map<string, string?>> rows = conn.query(q.sql, q.params);

// Non-SELECT: affected-row count.
int n = conn.execute("UPDATE users SET active = ? WHERE id = ?", ["1", "7"]);

int lastId = conn.lastInsertId();    // sequence/auto-increment — explicitly non-deterministic
conn.close();
```

Design notes:
- **`Db.connect` returns `Connection?`** (optional) and/or participates in the `throws E` error model
  (M-faults Slice 2 shipped) — a failed connect is the common case and should compose with `??`/`if-let`
  exactly like `File.read`'s `string?`.
- **`Connection` is a class instance** holding an opaque handle. This is novel: every existing native
  returns a *value* (`Value::Int/Str/List/...`). A live connection is a **resource** with identity and
  lifetime — it doesn't fit the closed `Value` enum (Int/Float/Bool/Str/Bytes/Decimal/Null/List/Map/Set/
  Instance/Enum/Closure). Options: (a) a magic `Instance` whose body the Rust legs refuse to operate on;
  (b) a new `Value::Resource` variant — a Value-enum change, heavier than any prior native. Either way
  this is a **bigger surface than a stateless native**, reinforcing "defer until designed."
- **Result rows = `List<Map<string, string?>>`** is the safest typed shape: PDO `FETCH_ASSOC` returns
  string-keyed associative arrays; typing every column value as `string?` sidesteps the float-divergence
  trap (no float ever crosses the boundary — the caller parses with `Convert.toInt`/`toFloat`). A typed
  row struct or `Map<string, Any>` needs the deferred dynamic `Any`/`Json` type or generics-over-rows.

---

## 6. New VM Op needed?

**No new `Op`.** Like every native, `Core.Db` dispatches through the existing `Op::CallNative(idx, argc)`
— no chunk.rs/exec.rs/compiler coupling. **However**, a `Connection` resource handle may force a
**`Value` enum change** (a `Value::Resource` variant, §5) which, while not an `Op`, is a heavier core
change than any native to date (the closed `Value` enum is a load-bearing invariant per
`docs/INVARIANTS.md`). The connection-as-`Instance` workaround avoids the Value change but is hacky.

---

## 7. Named determinism risks (all forcing Tier B)

1. **Live DB state** — same query, different rows across runs. The fundamental one; no golden exists.
2. **Unordered row/column results** — engine/storage-defined without `ORDER BY`; differs Postgres vs
   MySQL vs SQLite. Any fixture test MUST pin `ORDER BY`.
3. **Auto-increment / sequence / `lastInsertId`** — counters; non-reproducible.
4. **DB-side clocks** (`NOW()`, `CURRENT_TIMESTAMP`, default-`now` columns) — the clock trap.
5. **Float round-trip** — DB numerics → PHP echo's 14-digit vs Rust Ryū (known KNOWN_ISSUE). Mitigated
   by typing columns `string?` (no float crosses the boundary).
6. **NULL handling** — SQL NULL → PHP `null` → must map to `Value::Null` consistently; type as
   `string?`.
7. **Connection failure / latency / timeouts** — non-deterministic I/O; the `Transport`-trait quarantine
   (M6) is the right home.
8. **MySQL `caching_sha2_password` auth** — needs RSA in the handshake → collides with the no-hand-
   rolled-crypto policy *if* a Rust wire client were ever attempted (another reason the Rust legs can't
   do this without a forbidden dep).

---

## 8. Effort & recommendation

- **PHP transpile leg:** small (a handful of `php:` closures around PDO).
- **Rust legs:** **infeasible std-only** — no connect without a forbidden dependency (`unsafe` FFI or a
  driver crate); best case is a documented stub that errors. A real Rust leg = a from-scratch wire
  protocol (large milestone, still off-spine).
- **Core change:** likely a `Value::Resource` variant for the connection handle (heavier than any prior
  native).
- **Test harness:** `tests/db.rs` against `/stack` docker Postgres/MySQL fixtures with pinned `ORDER BY`
  goldens (PHP-leg-only).

**Recommendation: adopt-later (defer).** Concretely: ship the **pure `Core.Sql` builder first** (Tier A,
fully gateable — that's a separate, clearly-feasible spike), then design `Core.Db` as part of M6 (the
`Transport`-trait I/O quarantine) or an explicit M-Batteries DB slice. Do **not** ship now because:
1. It has no gateable surface (Tier B by every measure).
2. The Rust legs cannot execute it std-only — strictly weaker than the Process/Env quarantine precedent,
   where the Rust legs still produce real results.
3. It likely needs a `Value` enum change (resource handle) that deserves its own design.
4. Its value is small without the `Core.Sql` builder it's meant to consume (build that first).

Not **reject** because the transpile target is real (PDO is core under `php -n`, verified) and a DB API
is an inevitable, legible PHP-parity feature — it's a sequencing/scope call, not a dead end.

---

## std Rust APIs relied upon (in any future Rust-leg attempt)

- `std::net::TcpStream` (the only std path to a real DB, via a hand-rolled wire protocol — not
  recommended).
- `std::env` (for credentials, like `Core.Env`) — trivially available.
- No std DB/SQL API exists — this is the crux.

## Confidence: **high**

The zero-dep + `forbid(unsafe)` invariants (verified in `Cargo.toml`) make the std-only Rust-leg
infeasibility a hard, checkable fact, not a judgment. The PDO-under-`php -n` availability is verified
live (`var_dump` output above). The quarantine mechanism is verified in source
(`tests/differential.rs:916`, `src/native/mod.rs:62`). The only soft area is exactly *which* future
milestone owns it (M6 vs an M-Batteries DB slice) — a sequencing detail, hence the high (not certain)
grade.
