# Design — DB Execution (Tier-B)

**Stage 2 design.** Consumes the planned typed `Sql` query builder (#6 in the extended-modules
plan); transpiles execution to **PDO prepared statements**; honest gateability ≈ **0%** on the
byte-identity spine (the Rust legs cannot open a connection — no driver, `#![forbid(unsafe_code)]`),
so it is **Tier-B**: `pure:false`, quarantined from `tests/differential.rs`, transpiled to real PHP,
and fixture-tested in a dedicated `tests/db.rs` against the `/stack` docker Postgres/MySQL.

---

## 1. Verdict

**Tier-B.** Split the feature cleanly along the one axis that matters — *determinism / backend
symmetry*:

| Sub-feature | Tier | Why |
|---|---|---|
| `Sql` query builder (escaping, binding, parameterized fragments) | **A** (separate slice #6, already planned) | Pure string/value transformation; byte-identical on all three legs; gates normally. **This design assumes #6 ships first.** |
| `Db.connect` / `Connection` / `Statement` / `Row` value types + `execute`/`query` | **B** | Opening a connection is the single most non-deterministic surface (network, server state, row order, auto-increment ids, clock columns). The **Rust legs physically cannot do it** — std has no DB driver, and a pure-Rust driver would still need a live server. Quarantined + PHP-only fixture-tested. |

This is the **same two-tier split the developer already locked** for `Core.Process`/`Core.Env` and
for the M6 server (`Transport` behind a trait, quarantined out of the differential). DB execution is
the natural next inhabitant of that seam — it is *more* Tier-B than Process (Process at least runs on
the Rust legs; DB execution **cannot even run there** without a driver).

**Confidence: medium.** The mechanism (quarantine, PDO transpile, fixture harness) is high-confidence
— it mirrors shipped code (`process.rs`, `serve.rs`). The genuinely uncertain part is **how the Rust
legs behave when they hit `Db.connect`** with a closed `Value` enum that has no `Resource` variant
(§4) — that is the load-bearing design decision and the source of the open questions in §8.

---

## 2. The byte-identity argument (why it is NOT gateable)

The three-leg spine requires `run` ≡ `runvm` ≡ `php -n` on **stdout bytes**. DB execution breaks
this on **every one** of the three documented break-axes simultaneously:

1. **Non-determinism.** A query result depends on live server state, not program text: row order
   without `ORDER BY`, auto-increment / `SERIAL` ids, `now()`/`CURRENT_TIMESTAMP` columns, concurrent
   writers. No golden output exists.
2. **Backend asymmetry (the decisive one).** The PHP leg *can* run a query (PDO is core, present
   under `php -n`). The **Rust legs cannot** — `std` ships no MySQL/Postgres/SQLite client, and a
   hand-rolled wire-protocol client (a) is a multi-thousand-line undertaking, (b) still needs a live
   server, and (c) for TLS-required servers hits the project's one hard wall (no std TLS). So
   `run`/`runvm` have *nothing to compare against* the PHP leg's real rows.
3. **The hard wall (TLS) is in scope.** Production DBs commonly require TLS; the Rust legs have no
   TLS without a crate. Even http-only-style escapes don't help a binary wire protocol.

Because all three break-axes apply, **no amount of cleverness makes execution gateable** — it is
Tier-B by construction, exactly like `Core.Process`. The quarantine flag (`pure:false`) is the
existing, correct mechanism: `uses_impure_native` derives the impure-module set from the `pure` flag,
so a program that `import Core.Db;` is **automatically skipped** by both differential passes (lines
1004 and 1904) with **zero harness edits**. That seam was built for this.

**What IS gateable** and ships in the prerequisite slice #6: the `Sql` *builder* — `Sql.eq`,
`Sql.where`, parameter collection, identifier quoting — is pure value→value, byte-identical on all
three legs, and gates normally. The builder produces a `(sql_text, params)` pair; **execution** is the
only impure step. This keeps the injection-safety logic (the high-value, security-critical part) on
the gated spine, and quarantines only the irreducibly-live step.

---

## 3. Phorj-syntax API sketch

The builder (#6, Tier-A, gated) and execution (this slice, Tier-B, quarantined) compose:

```phorj
package Main;
import Core.Console;
import Core.Db;          // <- pure:false; importing this quarantines the program
import Core.Sql;         // <- pure builder (slice #6, gated)

// --- value types (all are ordinary Phorj classes; see §4) ---
// class Connection  { ... }      opaque handle wrapper
// class Statement   { ... }      a prepared statement bound to a connection
// class Row         { fields: Map<string, string?> ... }
// class ResultSet   { rows: List<Row>, affected: int ... }

function main() -> void {
    // connect — Tier-B: succeeds only on the PHP leg / fixture harness.
    // DSN is a plain string; credentials are explicit args (no ambient env unless via Core.Env).
    Connection conn = Db.connect("pgsql:host=localhost;dbname=app", "user", "pass");

    // Build a parameterized query with the gated Sql builder (no string concatenation).
    var q = Sql.select("users")
              .columns(["id", "name"])
              .whereEq("active", true)          // -> "active = ?", params=[true]
              .limit(10);                         // q : Query  (sql_text + params)

    // Execute — the impure step. Returns a ResultSet value.
    ResultSet rs = Db.query(conn, q);

    for (Row r in rs.rows) {
        // Row access is a typed Map lookup; columns are string? (nullable, DB reality).
        Console.println("{r.get(\"id\")}: {r.get(\"name\") ?? \"<null>\"}");
    }

    // A write returns affected-row count.
    var ins = Sql.insert("users").values(["name" => "Ada"]);
    int n = Db.execute(conn, ins);
    Console.println("inserted {n}");

    // Explicit lifecycle (no Drop-based close on the gated legs; see §4).
    Db.close(conn);
}
```

**Native signatures (registry entries, all `pure:false`):**

| `(module, name)` | params | ret |
|---|---|---|
| `Core.Db.connect` | `(string dsn, string user, string pass)` | `Connection` |
| `Core.Db.query` | `(Connection, Query)` | `ResultSet` |
| `Core.Db.execute` | `(Connection, Query)` | `int` (affected rows) |
| `Core.Db.queryRaw` | `(Connection, string sql, List<string> params)` | `ResultSet` |
| `Core.Db.close` | `(Connection)` | `void` |
| `Core.Db.transaction` | `(Connection, (Connection) -> bool)` | `bool` (HigherOrder; commit if the closure returns true, else rollback) |

`Connection`/`Query`/`Row`/`ResultSet` are **ordinary Phorj classes** (the public surface — Shape-A
style, mirroring M6 W1's `Request`/`Response`), defined in a small injected prelude (the `Core.Json`
injected-type precedent: inject the AST before `check`, gated on the import). They carry no special
runtime machinery on the gated path — see §4 for how the *opaque handle* lives inside `Connection`.

---

## 4. How a closed `Value` enum hosts a connection handle (the load-bearing decision)

`Value` is a **closed enum** (`Int/Float/Decimal/Bool/Str/Bytes/Unit/Null/List/Map/Set/Instance/Enum/
Closure`). There is **no `Resource` variant**, and adding one is undesirable: it would (a) need a
`Send`-incompatible payload that doesn't fit the `Rc`-heap discipline, (b) force every kernel/match in
both backends to handle a variant that **only the PHP leg can ever populate**, and (c) leak a
non-deterministic, non-`Clone`-friendly object into the value model that the byte-identity spine
assumes is pure data.

**Decision: do NOT add a `Resource` variant.** Instead, `Connection` is a normal `Value::Instance`
(an ordinary Phorj class) whose fields carry an *opaque token*, not a live handle:

- **PHP leg (the only leg that connects):** the transpiler emits `Connection` as a thin PHP class
  wrapping a real `\PDO`. `Db.connect` → `new \PDO($dsn, $user, $pass)` stored in the instance. This
  is where execution actually happens; PDO is core under `php -n`.
- **Rust legs (interpreter + VM):** `Db.connect` returns a `Value::Instance` of class `Connection`
  whose fields hold only a **handle id (int)** registered in a process-side connection table — the
  *same pattern `process.rs` already uses* for `PROCESS_ARGS` (a `RwLock`-guarded process global). The
  table maps `id -> Box<dyn DbBackend>` behind a **`DbBackend` trait** (the `Transport` seam, mirrored
  from `serve.rs`). The **default** Rust-side backend is `NullDbBackend`, which **faults cleanly**:
  `Db.query` on it returns a `FaultKind`-classified error `"Core.Db: no driver on this backend (Tier-B; run via the PHP transpile or a fixture backend)"`.

So on the Rust legs a DB program **type-checks, compiles, and runs up to the point of execution**,
then faults deterministically — it never silently differs. The byte-identity differential never sees
it (quarantined), and `tests/db.rs` injects a **fixture `DbBackend`** (an in-memory canned-result
backend, or a real connection to the `/stack` docker Postgres) so the Rust legs *can* be exercised
under a controlled, deterministic environment — exactly the `tests/process.rs` model.

This keeps `Value` closed, keeps `#![forbid(unsafe_code)]` intact (no FFI driver in the library), and
confines all the live, non-`Send`, non-deterministic machinery to (a) the PHP transpile target and (b)
a swappable trait object stored *outside* the value enum in a process table.

```rust
// src/native/db.rs  (sketch — lives outside the Value enum)
pub trait DbBackend {
    fn query(&mut self, sql: &str, params: &[Value]) -> Result<ResultSetData, String>;
    fn execute(&mut self, sql: &str, params: &[Value]) -> Result<i64, String>;
    fn close(&mut self);
}
struct NullDbBackend;            // default on the gated legs: every method faults cleanly
static DB_TABLE: RwLock<Vec<Box<dyn DbBackend>>> = ...;   // process global, like PROCESS_ARGS
pub fn install_db_backend(b: Box<dyn DbBackend>) -> i64 { /* tests/db.rs calls this */ }
```

**No new VM `Op` and no new `Value` variant are required.** All six natives are ordinary
`Op::CallNative` dispatches; `connect`/`query`/`execute`/`close` are `NativeEval::Pure` (they read
args + the process DB table), and `transaction` is `NativeEval::HigherOrder` (it invokes the
closure via the existing `ClosureInvoker`, exactly like `Core.List.map`). The connection handle is a
`Value::Instance` field (an `int` id), not a value-enum extension.

---

## 5. Exact PHP transpile target

`Core.Db` natives erase to **PDO** (core, present under `php -n`). The `Connection`/`Query`/`Row`/
`ResultSet` classes erase to plain PHP classes (the injected-prelude pattern), and the natives map to
PDO calls via the `php: fn(&[String]) -> String` facet:

```php
// Db.connect(dsn, user, pass)  ->
new \PDO($dsn, $user, $pass, [\PDO::ATTR_ERRMODE => \PDO::ERRMODE_EXCEPTION])

// Db.query(conn, q)  where q->sql is the builder's "SELECT ... WHERE x = ?" and q->params the binds:
(function($pdo, $sql, $params) {
    $st = $pdo->prepare($sql);          // PREPARED STATEMENT — the injection-safe path
    $st->execute($params);
    return $st->fetchAll(\PDO::FETCH_ASSOC);   // -> wrapped into ResultSet rows
})($conn->pdo, $q->sql, $q->params)

// Db.execute(conn, q)  ->  $st = $pdo->prepare($sql); $st->execute($params); return $st->rowCount();

// Db.transaction(conn, fn)  ->
(function($pdo, $fn) {
    $pdo->beginTransaction();
    try { $ok = $fn($pdo); if ($ok) { $pdo->commit(); } else { $pdo->rollBack(); } return $ok; }
    catch (\Throwable $e) { $pdo->rollBack(); throw $e; }
})($conn->pdo, $fn)
```

The builder (#6) already guarantees `$sql` is parameterized and `$params` is a positional bind list,
so the transpiled PHP **never string-concatenates user values** — prepared statements are the only
execution path. This is the security win the two-tier split buys: the gated builder is unit-tested for
escaping/binding on the spine; the impure executor only *runs* an already-safe `(sql, params)` pair.

Driver-specific DSN strings (`pgsql:`, `mysql:`, `sqlite:`) pass through verbatim — PDO is
driver-agnostic at the API level, so one transpile target covers all three SQL backends.

---

## 6. New VM Op / Value needed?

**None.**

- **No new `Value` variant** — the connection handle is a `Value::Instance` int-id field; the live
  object lives in a process table behind `DbBackend`, outside the enum (§4). This is the explicit,
  deliberate answer to the brief's question: a closed `Value` hosts a connection handle by *not*
  hosting it — it hosts an *opaque id* and keeps the live resource in a side table keyed by that id.
- **No new `Op`** — `connect/query/execute/queryRaw/close` are `Op::CallNative` (`NativeEval::Pure`);
  `transaction` is `Op::CallNative` (`NativeEval::HigherOrder`, reusing the shipped re-entrant
  `call_closure_value`/`run_until` invoker). No `exec_op`/`validate`/`stack_effect` triple-match edit.

This is the cleanest possible blast radius: purely additive registry entries + one new
`src/native/db.rs` leaf + an injected prelude + a new `tests/db.rs`. The four backends are untouched
except for the generic native-call path they already have.

---

## 7. Determinism risks (named)

1. **Row order without `ORDER BY`** — server-dependent; tests must always `ORDER BY` or sort rows
   before asserting. (Same class of risk as `Core.Env.all`'s OS-iteration-order, solved there by
   sorting.)
2. **Auto-increment / `SERIAL` ids, `now()` columns** — non-reproducible; fixtures must avoid
   asserting on generated ids/timestamps, or use a deterministic seed/`RETURNING` with known values.
3. **Server state leakage between tests** — `tests/db.rs` must run each case in a transaction that
   rolls back, or against a freshly-seeded schema (the `/stack` docker Postgres reset procedure).
4. **TLS-required servers** — the Rust legs cannot connect even with a real driver (no std TLS); the
   fixture backend therefore targets a **non-TLS local** docker Postgres/MySQL, and the in-memory
   canned backend for pure unit cases.
5. **PDO error-mode divergence** — PHP throws on SQL error (exception mode); the Rust fixture backend
   must surface the *same* `FaultKind` so error tests stay comparable to the eventual PHP run.
6. **Float/decimal column rendering** — DB numeric columns round-trip through string; reuse the
   M-NUM `decimal` discipline (rows are `string?`, parsed explicitly) to avoid float-formatting
   divergence (the documented `sqrt(2.0)` 14-digit PHP `echo` trap).

All six are *moot for the spine* (DB programs are quarantined) and are **fixture-harness concerns** —
they constrain how `tests/db.rs` is written, not the language.

---

## 8. Open questions for the developer

1. **Fixture backend choice:** in-memory canned-result `DbBackend` (zero deps, fully deterministic,
   but doesn't exercise real SQL) **vs** a live non-TLS `/stack` docker Postgres (real SQL, but needs
   the container up and a seed/reset step in CI)? Recommend **both**: canned for the gated-builder
   unit layer, docker-Postgres for an opt-in integration tier (`PHORJ_DB_DSN` env gate, skipped when
   unset — like `PHORJ_REQUIRE_PHP`).
2. **Connection lifecycle:** explicit `Db.close` only (sketched), or also a scope-bound
   `Db.withConnection(dsn, fn)` that guarantees close on the PHP leg via `finally`? The latter is more
   idiomatic and avoids leaked handles; the former is simpler. Recommend adding `withConnection` as
   the *blessed* form and keeping `connect`/`close` as the escape hatch.
3. **`Row` typing:** all columns `string?` (DB-faithful, forces explicit parse — sketched) **vs** a
   typed-row mapping driven by the `Sql` builder's column types? Typed rows are nicer but couple
   execution tightly to the builder and risk Rust/PHP coercion divergence. Recommend `string?`
   for v1.
4. **Does this slice ship at all before the `Sql` builder (#6)?** This design **hard-depends** on #6
   for the injection-safe `(sql, params)` pair. Confirm #6 lands first (the plan orders it that way).
5. **Should `Db` even be a *runnable example* or only a README walkthrough?** Per the "examples ship
   with features" rule, but quarantined Tier-B features (Process) ship a **walkthrough README + a
   non-gated companion `.phg`**, not a gated example. Recommend the Process model: a
   `examples/db/` walkthrough, fixture-tested in `tests/db.rs`, **not** added to the gated glob.
6. **Transaction closure on the Rust legs:** with `NullDbBackend`, `transaction` faults at the first
   query inside the closure. Acceptable (deterministic fault), or should `transaction` itself fault
   *before* invoking the closure so the error message names the transaction? Minor; recommend
   faulting at first query (lets the closure run pure logic up to the DB touch).

---

## 9. Effort & feasibility

- **Effort: medium.** ~1 new leaf (`src/native/db.rs`, ~6 natives + `DbBackend` trait + process
  table + `NullDbBackend`), an injected prelude for the 4 classes (Json precedent), PDO transpile
  mappings (6 `php:` closures), a new `tests/db.rs` fixture harness, and a walkthrough example. No
  backend/Op/Value changes. The bulk of the *value* (injection-safe SQL) lives in the prerequisite
  Tier-A builder slice #6, not here.
- **Honest gateability ≈ 0%** on the byte-identity spine (by construction — Tier-B). **Feasibility of
  the Tier-B mechanism ≈ 85%**: every piece has a shipped precedent (`process.rs` quarantine,
  `serve.rs` Transport trait, `json.rs` injected types, `list.rs` HigherOrder + ClosureInvoker). The
  15% uncertainty is the fixture-backend ergonomics (Q1) and whether the Rust-leg `NullDbBackend`
  fault story satisfies the "deterministic up to the live boundary" goal in practice.
