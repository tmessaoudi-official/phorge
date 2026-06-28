# STAGE 4 — DESIGN: `Core.Sql` — the typed, injection-safe query builder (the pure half of DB)

**Tier: A (pure, deterministic).** Confidence: **high** on the byte-identity spine and the std-only
claim; **medium** on the bind-value ergonomics decision (resolved below by reusing the *already-shipping*
`Json` enum). Net feasibility (this design): **~88%** — up from the Stage-2b refutation's ~78%, because
the two compile-blockers it found (heterogeneous bind list rejected at `checker/expr.rs:686`; no
list-printing surface) are both closed here without inventing any new language feature.

This document specifies: (1) the Phorj API; (2) *why injection is impossible by construction*; (3) the
exact `(sql, params)` shape produced; (4) the PHP transpile target (PDO prepared statements); (5) the
byte-identity plan; (6) dialect concerns; (7) the build plan that compiles **today**.

---

## 0. The two refutations from Stage 2b, and how this design answers them

The feasibility spike was sound on determinism but its *stated API does not compile*. Two hard
compile-blockers (both Verified against the live codebase by Stage 2b):

| Blocker (Stage 2b) | Live evidence | This design's answer |
|---|---|---|
| **Heterogeneous bind list `[18, true, "x"]` is a type error** | `src/checker/expr.rs:686` — `"list elements must share one type; found `int` and `bool`"` | **Bind values ARE `Json` enum values** — `List<Json>`. The `Json` enum (`Int`/`Float`/`Bool`/`Str`/`Null`/…) is *already injectable* (`JSON_PRELUDE`, `src/cli/mod.rs:294`) and already models every scalar a SQL bind needs. `[Json.Int(18), Json.Bool(true), Json.Str("x")]` is a **homogeneous `List<Json>`** — it type-checks today. **Zero new injected type.** |
| **The gating example can't print a `List` (`Debug.dump` doesn't exist; `as_display` returns `None` for `List`)** | `src/value.rs:254` `_ => None`; `src/checker/expr.rs:540` `"type `list` cannot be interpolated"` | The example prints only **`q.sql()` (a `string`)** and a **hand-rolled per-bind line** via `match` over the `Json` variants (the exact pattern `examples/guide/json.phg`'s `shapeOf` already ships). No `Debug.dump` dependency. |

The net effect: the public API and its differential-gated example are writable **with only features that
ship today** (injected enum/class, `clone-with`, `match`, `Core.Text.join`, `List<T>`).

---

## 1. The framing that decides everything: PARAMETERIZE, NEVER INLINE

There are two builder shapes. Only one is both *secure* and *Tier-A-deterministic* — and they are the
**same** shape:

1. **Inline-escaped** (REJECTED): `Sql.quoteString("o'brien") -> 'o''brien'`, splice the escaped literal
   into the SQL text. This is the classic injection surface *and* a determinism trap: the only correct
   PHP escape is `PDO::quote`, which (a) is **absent under `php -n`** — the oracle runs `php -n`
   ([[transpile-no-ini-extensions]]) — and (b) is **connection-charset-dependent → non-deterministic**.
   A hand-rolled escaper would have to byte-match PDO across dialects, which is exactly the divergence
   the byte-identity spine forbids. **No `quoteString`/`escapeString` in the public API. Ever.**

2. **Parameterized** (ADOPTED): every user value becomes a positional placeholder `?` in the SQL text
   and is collected, *as a value, never stringified*, into an ordered params list. The literal **never
   enters the SQL string**, so there is no escaping algorithm to get wrong and none to diverge.

> **Injection is impossible by construction:** the SQL string is assembled *only* from (a) developer-
> authored fragments and keywords, and (b) `?` placeholders the builder emits itself. Bind *values* go
> into a separate `List<Json>` that is handed to the driver's prepared-statement binder. A value can
> never be parsed as SQL because it is never concatenated into SQL. This is not "escaping done well" —
> it is "escaping not needed," the only honest meaning of "safe by construction." `[Inferred: this is
> the PDO/Doctrine-DBAL/squirrel consensus and the only design with no Rust↔PHP escaping path to
> diverge — see feasibility-Sql.md §1, byte-identity-verified in refutation-Sql.md.]`

---

## 2. Bind values are `Json` — the keystone decision

The hardest real constraint (Stage 2b's P0) is that Phorj `List<T>` is homogeneous. A SQL query binds
mixed scalars in one clause (`WHERE age > ? AND name = ?` ⇒ `[18, "alice"]`). The clean, *already-shipping*
answer:

```phorj
// The Json enum is injected on `import Core.Json` (JSON_PRELUDE, src/cli/mod.rs:294):
//   enum Json { Null(), Bool(bool value), Int(int value), Float(float value),
//               Str(string value), Arr(List<Json> items), Obj(Map<string,Json> entries) }
```

A SQL bind is exactly one of `Null | Bool | Int | Float | Str` — the scalar subset of `Json`. So:

- **Bind type = `Json`.** A params list is `List<Json>` — **homogeneous, type-checks today.**
- `Core.Sql` re-injects the *same* `Json` enum it needs (idempotent: the injector already no-ops when
  the enum is already declared, see `already_declared` in `inject_json_prelude`), so `import Core.Sql`
  alone makes `Json.Int(18)` available without forcing the user to also `import Core.Json`.
- **Ergonomic sugar (designed, optional v1.1):** thin static constructors on `Sql` so call sites read
  naturally instead of `Json.Int(18)`:
  - `Sql.int(int v) -> Json`, `Sql.str(string s) -> Json`, `Sql.bool(bool b) -> Json`,
    `Sql.float(float f) -> Json`, `Sql.null() -> Json`.
  - These are one-line wrappers (`return Json.Int(v);`) in the injected prelude — pure, byte-identical,
    no native. v1 can ship with bare `Json.Int(...)`; the `Sql.*` wrappers are a readability layer.

> **Why not a bespoke `Bind` enum (the spike's §6 fallback)?** Reusing `Json` means (a) zero new injected
> type, (b) the value↔PHP rendering is *already proven byte-identical* (json's Int/Float/Bool/Str arms
> are oracle-tested, see `examples/guide/json.phg`), and (c) it composes with `Core.Json.parse` for the
> common "bind a parsed JSON payload" case. `[Speculative: bespoke-Bind would also work but duplicates
> a shipping type.]`

---

## 3. Phorj API (compiles with today's surface)

```phorj
package Main;
import Core.Console;
import Core.Sql;        // injects the Query/Sql classes AND (idempotently) the Json enum

function main(): void {
    Query q = Sql.select(["id", "name", "email"])
        .from("users")
        .where("age > ?",   [Sql.int(18)])
        .where("active = ?",[Sql.bool(true)])
        .andWhere("name = ?", [Sql.str("alice")])
        .orderBy("name", "ASC")
        .limit(10)
        .offset(20);

    Console.println(q.sql());
    // SELECT "id", "name", "email" FROM "users"
    //   WHERE age > ? AND active = ? AND name = ? ORDER BY "name" ASC LIMIT 10 OFFSET 20
    // (assembled on one line — see §5 for exact bytes)

    // params are List<Json> — printed via a hand-rolled per-bind renderer (no Debug.dump):
    List<Json> ps = q.params();
    for (Json b in ps) {
        Console.println(renderBind(b));   // "int:18", "bool:true", "string:alice"
    }
}

function renderBind(Json b): string {
    return match b {
        Null()   => "null",
        Bool(v)  => "bool:{v}",
        Int(v)   => "int:{v}",
        Float(v) => "float:{v}",
        Str(v)   => "string:{v}",
        Arr(xs)  => "array",       // not a valid scalar bind, but match must be exhaustive
        Obj(es)  => "object",
    };
}
```

### 3.1 Surface (the injected `Sql` + `Query` classes)

`Sql` — the entry factory (static methods only):

| Method | Signature | Notes |
|---|---|---|
| `select` | `Sql.select(List<string> cols) -> Query` | empty list ⇒ `SELECT *` |
| `insert` | `Sql.insertInto(string table) -> Query` | pair with `.values(...)` |
| `update` | `Sql.update(string table) -> Query` | pair with `.set(...)` |
| `delete` | `Sql.deleteFrom(string table) -> Query` | pair with `.where(...)` |
| `int/str/bool/float/null` | `Sql.int(int) -> Json` … | bind-value constructors (sugar over `Json.*`) |

`Query` — the chained, **immutable** (clone-with) builder:

| Method | Signature | Produces |
|---|---|---|
| `from` | `from(string table) -> Query` | sets FROM table (SELECT/DELETE) |
| `columns` | `columns(List<string> cols) -> Query` | override projection |
| `where` | `where(string cond, List<Json> binds) -> Query` | first/AND predicate + appends binds **in order** |
| `andWhere` | `andWhere(string cond, List<Json> binds) -> Query` | alias of `where` after the first (reads better) |
| `orWhere` | `orWhere(string cond, List<Json> binds) -> Query` | OR-group predicate (parenthesized, see §6) |
| `join` | `join(string table, string on) -> Query` | INNER JOIN … ON … (no binds; ON is identifier-only) |
| `leftJoin` | `leftJoin(string table, string on) -> Query` | LEFT JOIN |
| `set` | `set(string col, Json val) -> Query` | UPDATE assignment; `col` quoted as identifier, value bound |
| `values` | `values(Map<string,Json> row) -> Query` | INSERT columns+binds (insertion-ordered Map ⇒ stable) |
| `groupBy` | `groupBy(List<string> cols) -> Query` | GROUP BY (identifiers) |
| `having` | `having(string cond, List<Json> binds) -> Query` | HAVING predicate + binds |
| `orderBy` | `orderBy(string col, string dir) -> Query` | dir validated to `ASC`/`DESC` (see §6) |
| `limit` | `limit(int n) -> Query` | **inline int** (drivers reject bound LIMIT; int render is deterministic) |
| `offset` | `offset(int n) -> Query` | inline int |
| `sql` | `sql() -> string` | the assembled SQL template |
| `params` | `params() -> List<Json>` | bind values, in placeholder order |

> All mutators **return a fresh `Query` via `clone-with`** (`this with { wheres = …, binds = … }`),
> verified-shipping syntax (`examples/guide/clone-with.phg`). No in-place mutation needed; chaining is
> referentially transparent — a half-built `Query` is reusable. `[Verified: clone-with is shipped and
> byte-identity-gated.]`

### 3.2 The identifier-quoting safe-by-default rule

`select`/`from`/`orderBy`/`groupBy`/`columns`/`join`/`set` take **identifier** strings (table/column
names), which the builder **quotes deterministically** (ANSI `"id"` with `"`→`""` doubling, §6). `where`/
`having` take a **developer-authored condition fragment** containing `?` placeholders — the developer owns
that fragment's SQL but **never interpolates a user value into it** (that's what the `binds` list is for).
This split is the whole safety model: identifiers are quoted by us; values are bound; only static SQL
keywords/operators are author-supplied.

---

## 4. The produced `(sql, params)` pair — exact shape

The builder's terminal state is a **deterministic pure function of its typed inputs**:

- `sql() : string` — the SQL template with `?` positional placeholders for every bound value, identifiers
  ANSI-quoted, LIMIT/OFFSET inlined as decimal integers.
- `params() : List<Json>` — bind values **in the exact left-to-right order their `?` appears in the SQL**.

Order invariant (the one ordering concern): every mutator that appends a clause fragment to the SQL also
appends its binds to the params list **in the same call**, so placeholder order == bind order by
construction. `List` is insertion-ordered `Rc<Vec>` ([[value-kernels-single-sourced]]) — identical across
all three legs. INSERT `values(Map)` relies on the **insertion-ordered `Value::Map`** (Map discipline,
R1) so column order == placeholder order == bind order deterministically.

Worked example (the §3 query):

```
sql    = SELECT "id", "name", "email" FROM "users" WHERE age > ? AND active = ? AND name = ? ORDER BY "name" ASC LIMIT 10 OFFSET 20
params = [Json.Int(18), Json.Bool(true), Json.Str("alice")]
```

This pair is what a later **Tier-B `Core.Db`** consumes: `db.query(q.sql(), q.params())`. The builder
**never executes** — execution (connection, network, result set) is the non-deterministic half,
quarantined behind the M6 `Transport` trait, fixture-tested *outside* `differential.rs`, exactly as the
`Process`/`Env` precedent (`pure: false`). `Core.Db` is **out of scope** here.

---

## 5. PHP transpile target — PDO prepared statements

Because `Core.Sql` is **injected Phorj classes**, the transpiler emits their method bodies as ordinary
PHP — there is **no SQL-specific PHP builtin on the build path**. The whole module reduces to string
concatenation (`.`), `implode`, `str_replace`, and array appends — all PHP **core**, all surviving
`php -n` ([[transpile-no-ini-extensions]]).

### 5.1 The builder's own emission (build path — pure, byte-identity-gated)

| Phorj | PHP |
|---|---|
| `Text.join(parts, ", ")` | `implode(', ', $parts)` (`Text.join → implode`, `src/native/text.rs:76`) |
| identifier quote: `"\"" + Text.replace(id, "\"", "\"\"") + "\""` | `'"' . str_replace('"', '""', $id) . '"'` |
| `this with { wheres = … }` | clone-with's emitted PHP (shipping) — fresh array assembly |
| `q.params()` returns `List<Json>` | a PHP `array` of the emitted `Json`-enum objects (json's variants transpile already-gated) |
| `match` over a `Json` bind | PHP `match` (native, floor 8.5) over the mangled enum class |

> Reserved variant names (`Int`/`Float`/`Bool`/`Null`) are mangled invisibly by the transpiler
> (`php_variant_name`, see [[core-json-and-injected-types]] and `examples/guide/enum-reserved-variants.phg`).
> This is **already solved** for `Json` — `Core.Sql` inherits it for free.

### 5.2 The Tier-B execution target (documented, NOT in this module)

When `Core.Db` later executes the pair, the transpiled PHP is canonical PDO:

```php
$pdo  = new PDO($dsn, $user, $pass, [PDO::ATTR_ERRMODE => PDO::ERRMODE_EXCEPTION]);
$stmt = $pdo->prepare($query->sql());      // "... WHERE age > ? AND active = ? ..."
$stmt->execute($query->params());          // [18, true, "alice"] — bound positionally, never interpolated
$rows = $stmt->fetchAll(PDO::FETCH_ASSOC);
```

The `?` template + positional `execute([...])` array is **the literal PDO prepared-statement contract** —
the builder's output is a drop-in for `PDO::prepare`/`execute`. PDO is a driver extension (absent under
`php -n`), which is why it lives only in the deferred Tier-B module and **never** on the byte-identity
build path. `[Inferred: PDO positional-`?` binding is the PHP-idiomatic execution target; matches the
"every feature maps to idiomatic PHP" transpile contract.]`

---

## 6. Byte-identity plan

**The byte-identity guarantee is free by construction: the build path emits only `string` + `List<Json>`,
both already byte-identical primitives on the existing spine, and the PHP leg runs the *same* Phorj
string code (transpiled), so there is no second implementation to diverge.** Concretely:

1. **Placeholders are literal `"?"`** the Phorj code writes — identical on all three legs, no surface.
2. **Identifiers are quoted by a literal Phorj transform** (`"`-wrap + `str_replace('"','""')`) — pure,
   ASCII, core-only, byte-level (no mbstring). Identical by construction.
3. **Bind values are carried, never formatted** — a float bind is stored as `Json.Float`, never rendered
   into the SQL string, so the Ryū-vs-PHP-`precision=14` float divergence ([[php-leg-outside-correctness-loop]])
   **never touches the build path**. (It would only appear if a Tier-B `Core.Db` stringified a bind —
   out of scope.)
4. **LIMIT/OFFSET are inlined as `int`** — `int` renders identically (`n.to_string()` vs PHP `(string)int`,
   no float, no locale, `src/value.rs`), and many drivers reject bound LIMIT, so inlining is also the
   correct PDO choice.
5. **The gating example prints only `string` (`q.sql()`) and a `match`-rendered per-bind line** — a printed
   `int`/`bool`/`string` is reconciled by the `__phorj_str` runtime helper
   ([[php-leg-outside-correctness-loop]]); a `List` is **never** printed directly (avoids the
   `as_display→None` / non-interpolatable-list trap, `src/value.rs:254`).

**Gating:** `examples/sql/builder.phg` is globbed by `tests/differential.rs` (`examples/**/*.phg`) and
asserts `run ≡ runvm ≡ real-PHP-8.5` byte-for-byte, run with `PHORJ_PHP=…php-8.5.7 PHORJ_REQUIRE_PHP=1`
([[php-transpile-floor-84]]). Because the build path has no clock/RNG/float-format/map-order/object-id
surface, the assertion is structurally safe.

**No new VM `Op`.** The builder is injected classes + (at most) `Pure` natives → `Op::CallNative`.
Confirmed against the registry model ([[op-variant-match-coupling]] avoided entirely; every recent module
— Json/Decimal/Convert — added zero Ops).

---

## 7. Dialect concerns (flagged, with the v1 pins)

SQL is not one language. The pins below are deterministic by construction; each variant is a **pure** v2
knob (a `Dialect` enum the builder branches on — still no native, still byte-identical):

| Concern | v1 pin (deterministic default) | Portability | v2 variant |
|---|---|---|---|
| **Placeholder** | `?` positional (PDO default, ANSI, MySQL/SQLite/pgsql all accept) | universal via PDO | `:name` named placeholders (pure: append `:nN` + a `Map<string,Json>` param bag) |
| **Identifier quote** | ANSI double-quote `"id"`, `"`→`""` doubling | pgsql, SQLite, ANSI-mode MySQL | `` `id` `` (MySQL backtick, `` ` ``→` `` ` ``), `[id]` (T-SQL) — a `Dialect` enum branch |
| **LIMIT/OFFSET** | `LIMIT n OFFSET m` (inline int) | MySQL, pgsql, SQLite | T-SQL `OFFSET … FETCH NEXT …`; Oracle `FETCHROWS` — dialect branch |
| **Boolean bind** | bound as `Json.Bool` → PDO binds PHP `bool` (1/0 on MySQL, `t/f` on pgsql) | driver handles it | n/a — value-level, driver's job |
| **`orWhere` grouping** | wrap OR-groups in parens: `(a = ? OR b = ?)` AND-joined to the rest | universal | n/a (precedence-safe default) |
| **`orderBy` direction** | validate `dir` ∈ {`ASC`,`DESC`} (case-insensitized to upper); reject else | universal | n/a (closes a direction-injection footgun) |
| **Casing of keywords** | UPPERCASE keywords (`SELECT`/`FROM`/`WHERE`) | cosmetic | n/a |

> **Named footgun closed:** `orderBy(col, dir)` and `set(col, …)`/`join(table, on)` take **identifiers**,
> not value fragments — so the only place a developer writes raw SQL is `where`/`having` *conditions*,
> and those carry their values exclusively through the `binds` list. `orderBy`'s `dir` is validated to an
> allowlist (`ASC`/`DESC`) so it can't smuggle SQL. There is **no API path** that interpolates a user
> value into the SQL string. `[Inferred: closes the classic "ORDER BY {userInput}" injection class.]`

> **`join … ON` caveat (Verified-by-reasoning):** the `on` argument is an identifier-comparison fragment
> (`"users"."id" = "orders"."user_id"`); v1 takes it as a developer-authored string (like `where`'s
> condition) and **does not** bind into it. A future typed `on(col, col)` form would quote both sides.
> Documented as a v1 limitation, not an injection hole (it's author-static, never user-data).

---

## 8. Build plan (compiles today; small–medium)

1. **`SQL_PRELUDE`** const + **`inject_sql_prelude`** in `src/cli/mod.rs`, mirroring `inject_json_prelude`
   — **generalize the prelude injector to carry `Item::Class` as well as `Item::Enum`** (today it
   `find`s only `Item::Enum`; extend to push all prelude items in order). The prelude declares the
   `Sql` + `Query` classes (pure Phorj method bodies: `clone-with` returns, `Core.Text.join`, ANSI
   quoting, `match`-over-`Json` in render). It also re-declares the `Json` enum **idempotently** (the
   existing `already_declared` guard makes co-import safe). Wire `inject_sql_prelude` into the
   `check_and_expand` chokepoint alongside the json/rounding injectors (`src/cli/mod.rs:378`).
2. **(Optional) `Pure` natives** *only* if a hot helper is cleaner in Rust than Phorj (e.g. an
   identifier-quote native) — **not required**; the pure-Phorj path is preferred (zero Rust↔PHP
   divergence surface). If added, one `src/native/sql.rs` leaf, `NativeEval::Pure`, `pure: true`, a
   `php` closure mapping to `str_replace`/`implode`. **No new `Op`.**
3. **`examples/sql/builder.phg`** — the §3 program (SELECT + INSERT + UPDATE + DELETE coverage, mixed
   binds via `Json`, per-bind render via `match`) + an `examples/README.md` entry (index + coverage
   matrix), in the **same change** (the examples-ship-with-features rule, [[examples-ship-with-features]]).
   Auto-gated by the `examples/**/*.phg` glob.
4. **Round-trip the floor:** `PHORJ_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php PHORJ_REQUIRE_PHP=1
   cargo test --workspace` ([[php-transpile-floor-84]]).

**Effort:** small–medium. The materially-new piece (Stage 2b's P1) is **injecting a method-bearing class
pair** — no shipped prelude does this yet (Json/RoundingMode are bare enums). Risk is low (it's ordinary
Phorj that already compiles standalone), but it must be proven: the de-risking step is to write the
`Query`/`Sql` classes as a *normal* user file first, gate it green, then lift it verbatim into
`SQL_PRELUDE`. `[Inferred: the prelude is just a string of Phorj source lex-parsed at inject time, so any
program that compiles standalone compiles as a prelude — verified mechanism in inject_json_prelude.]`

---

## 9. Named determinism risks (each controlled)

1. **Inline-escaping divergence (headline trap) — ELIMINATED BY DESIGN** (§1). No `quoteString`; PDO never
   on the build path (and absent under `php -n`).
2. **Bind-order ↔ placeholder-order skew** — controlled: SQL fragment + binds appended in the same mutator;
   `List`/`Map` insertion-ordered `Rc<Vec>`.
3. **Identifier-quote dialect drift** — controlled: pinned literal ANSI `"`+`""` in Phorj; no driver call.
4. **Heterogeneous bind typing** — **closed** by `List<Json>` (§2); no `Any`, no bespoke `Bind`.
5. **Float bind rendering** — N/A on build path (carried, never formatted); LIMIT/OFFSET pinned `int` (§6).
6. **mbstring absence** — N/A: identifiers/SQL ASCII; `str_replace`/`implode` byte-level core.
7. **List-printing in the example** — closed by the hand-rolled `match`-over-`Json` renderer (§3), never
   printing a `List` directly (avoids `as_display→None`, `src/value.rs:254`).
8. **`orderBy` direction / identifier injection** — closed by the allowlist + identifier-quoting split (§7).

---

## 10. Pairing note

`Core.Sql` (Tier A, this design) is the deterministic **front half**: a pure function from typed inputs to
a `(sql: string, params: List<Json>)` pair — independently useful (generate/log SQL, hand to any driver)
and fully byte-identity-gated. `Core.Db` (Tier B, `pure: false`, M6 `Transport`-quarantined,
fixture-tested *outside* `differential.rs`) is the deferred **execution half** that consumes the pair via
PDO `prepare`/`execute`. Ship `Core.Sql` standalone now; `Core.Db` follows the `Process`/`Env`
quarantine precedent later.
