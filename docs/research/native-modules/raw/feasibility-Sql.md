# Feasibility Spike — `Core.Sql` (typed, injection-safe query BUILDER)

**Verdict: ADOPT (Tier A, pure). Feasibility ~92%. Confidence: high.**
The hypothesis holds and is *stronger* than stated: the pure half of "DB" is not merely feasible — it
needs **zero native functions, zero PHP runtime helpers, and zero new VM `Op`** if the builder emits a
**parameterized statement** (`{sql: string, params: List}`) rather than inlining escaped literals.
The entire module can ship as **injected Phorj classes** (the `Json`/`RoundingMode` injected-type
pattern, `cli::inject_*_prelude`) whose methods are written in ordinary Phorj over already-shipped
`Core.Text`/`Core.List` ops. Byte-identity is then *free by construction* — there is no Rust-vs-PHP
escaping path to diverge.

---

## 1. The framing that decides everything: parameterize, never inline

The prior-art digest offers two builder shapes:

1. **Inline-escaped:** `Sql.quoteString("o'brien")` → `'o''brien'`, splice literals into the SQL text.
2. **Parameterized:** `.where("age > ?", [18]).build()` → `{ sql: "... WHERE age > ?", params: [18] }`.

**Shape 2 is the only Tier-A-clean design, and it is also the only *correct* design** (it is what
"injection-safe by construction" actually means — the literal never enters the SQL string, so there
is no escaping to get wrong). This is not a trade-off; the secure design and the deterministic design
are the same design. PDO/Doctrine DBAL/squirrel all converge here. **Decision: parameterized output
only. No `quoteString`/`escapeString`/`PDO::quote` in the public API.** (Identifier quoting is a
separate, safe, deterministic concern — see §4.)

Consequence for determinism: **the builder produces a `string` (the SQL template with `?`/`:name`
placeholders) plus a `List` of bind values.** A `string` and a `List<Value>` are already in the closed
`Value` enum and already round-trip byte-identically across all three legs (every shipped example
proves this). There is **no float formatting, no escaping, no locale, no clock, no RNG, no map
iteration** anywhere on the build path. The single ordering concern (placeholder order must match bind
order) is handled by Phorj's insertion-ordered `List` — the builder appends to the SQL and the bind
list in lockstep.

---

## 2. std-only feasibility — TRIVIAL (it's all Phorj string building)

The builder is `String` concatenation and `Vec` appends. Rust std APIs relied on: **none beyond what
already ships** — because the recommended implementation writes the builder *in Phorj*, not Rust. The
methods use `Core.Text` (`Text.join`, already shipped) and `Core.List` (`append`/spread) and ordinary
`+` string concat. If any helper must be a native (it need not), it is a `Pure(fn(&[Value], &mut
String))` over `Value::Str`/`Value::List` — the same shape as every `text.rs` body (`String::push_str`,
`<[_]>::join`). **No external crate, no TLS, no I/O, no clock.** `[dependencies]` stays empty.

---

## 3. Tier classification — Tier A (pure), unambiguous

| Concern | Where it lives | Tier |
|---|---|---|
| `select/from/where/join/orderBy/limit` → SQL template `string` | builder methods (pure Phorj) | **A** |
| bind values collected into `List` | builder state | **A** |
| identifier quoting (`"col"` / `` `col` ``) | pure deterministic string transform | **A** |
| **opening a connection / executing** | `Core.Db` — `pure: false`, M6 `Transport` quarantine | **B (separate module, out of scope)** |

The builder is the textbook Tier A case: a deterministic function from typed inputs to a `(string,
List)` pair. It pairs with a future `Core.Db` (Tier B) exactly as the digest predicts —
`db.query(q.sql(), q.params())` — but `Core.Db` is explicitly **not** this spike.

---

## 4. Byte-identity strategy

**Strategy: the build path produces only `string` + `List`, which are byte-identical primitives by the
existing spine. The PHP leg never runs any SQL-specific code at build time — the transpiled program
just concatenates the same strings and builds the same array.** This is the strongest possible
byte-identity guarantee: there is no second implementation to diverge.

Two sub-concerns, both deterministic:

- **Placeholder dialect.** Pin **`?` positional placeholders** (PDO default, ANSI, MySQL/SQLite/pgsql
  all accept via PDO emulation; `:name` named placeholders are a v2 add). Pinned identically in all
  three legs because it is a literal `"?"` the Phorj code writes. No divergence surface.
- **Identifier quoting.** Pin **ANSI double-quotes** `"ident"` with `"` → `""` doubling as the default
  (portable to pgsql/sqlite/ANSI MySQL; a `.dialect(MySql)` backtick variant is a clean v2 enum knob).
  This is `Text.replace(id, "\"", "\"\"")` wrapped in quotes — pure, no PHP `quote_identifier`, no
  driver call. Reject any path that calls PDO at build time (PDO is absent under `php -n` anyway — see
  §5).

**Result: byte-identity is free.** The differential harness globs `examples/sql/*.phg`; the example
prints `query.sql()` and `Debug.dump(query.params())` (or a hand-rolled join) and all three legs emit
identical bytes because they all ran the same Phorj string code.

---

## 5. Exact PHP transpile target

Because the builder is **injected Phorj classes**, the transpiler emits the classes' methods as
ordinary PHP — `string` concatenation with `.`, `array` appends, a `Text.join` → `implode`. **There is
no SQL-specific PHP builtin in the emission at all.** Specifically:

- `Sql.select([...])` → ordinary PHP constructor/static call on the emitted `Sql` class.
- `.where("age > ?", [18])` → `$this->wheres[] = ...; $this->binds = array_merge($this->binds, [18]);`
- `.build()` → returns a PHP value (an instance / associative array) with the assembled string + array.
- identifier quoting → `'"' . str_replace('"', '""', $id) . '"'` (core, survives `php -n`).
- `Text.join(parts, " AND ")` → `implode(' AND ', $parts)` (`implode` is core).

**`php -n` safety: VERIFIED-by-reasoning.** Everything reduces to `implode`, `str_replace`, `.`, and
array ops — all PHP **core**, none ext-dependent. **PDO is a driver/ext and is correctly NOT on the
build path** (it would only appear in the deferred Tier-B `Core.Db`). No mbstring (identifiers/SQL are
ASCII byte-level). No `hash`/`filter`/`intl`. The one trap to avoid is precisely the rejected
inline-escape design (`PDO::quote` needs a live PDO connection — impossible under `php -n` and
non-deterministic anyway). Parameterization sidesteps it entirely.

---

## 6. Phorj API sketch (parameterized, chained, injected classes)

```phorj
import Core.Sql;
import Core.Console;

// `Query` (and `Sql`) are INJECTED on `import Core.Sql` — the Json/RoundingMode pattern.
fn main() -> void {
    var q = Sql.select(["id", "name"])
        .from("users")
        .where("age > ?", [18])
        .where("active = ?", [true])
        .orderBy("name")
        .limit(10)
        .build();

    Console.println(q.sql());
    // => SELECT "id", "name" FROM "users" WHERE age > ? AND active = ? ORDER BY "name" LIMIT 10
    Console.println(Debug.dump(q.params()));   // => [18, true]
}
```

Injected prelude (sketch — pure Phorj, no natives):

```phorj
class Query {
    private List<string> cols;
    private string table;
    private List<string> wheres;
    private List params;        // bind values, insertion-ordered
    private List<string> orders;
    private int? lim;

    function from(string t) -> Query { ... returns clone-with table }
    function where(string cond, List binds) -> Query { append cond + binds }
    function orderBy(string col) -> Query { ... }
    function limit(int n) -> Query { ... }
    function build() -> Built { /* assemble SQL via Text.join, carry params */ }
    function sql() -> string { ... }
    function params() -> List { ... }
}
class Sql { static function select(List<string> cols) -> Query { ... } }
```

Notes on the sketch vs. current language surface (each is a real check before building):
- **Immutability/mutation:** chained builders need either `clone-with` returns (shipped, M-mut) or
  mutable fields (shipped). Either works; `clone-with` is the more idiomatic, byte-identity-proven path.
- **`List` (untyped bind list):** `params: List` holds mixed `Value`s. The collections that ship are
  `List<T>`; a heterogeneous bind list wants `List<Any>`/a `Bind` sum type. **This is the one genuine
  language gap** — see §8. Mitigation for v1: a `Bind` enum (`I(int)|S(string)|B(bool)|Nul()`), or
  ship after the dynamic-`Any` work the digest already flags as blocking `core.json` historically (now
  resolved via the injected `Json` enum — the same trick applies: a `Bind` injected enum).

---

## 7. New VM Op needed? — NO

The builder is injected Phorj classes + (optionally) `Pure` natives → `Op::CallNative` at most. **No
new `Op`.** This avoids the three-coupled-match cost (`chunk.rs` validate, `vm/exec.rs` exec_op,
`compiler` stack_effect) entirely. Confirmed against the registry model in `src/native/mod.rs`
(every recent module — Json, Decimal, Convert — added zero Ops).

---

## 8. Named determinism risks (and why each is controlled)

1. **Inline escaping divergence (the headline trap) — ELIMINATED BY DESIGN.** Rejecting
   `quoteString`/`PDO::quote` means there is no Rust-vs-PHP escaping algorithm to disagree. Hard-won:
   PDO is unavailable under `php -n` *and* `PDO::quote` is connection-dependent (charset) and
   non-deterministic. Parameterization is the only Tier-A path.
2. **Bind-order vs placeholder-order skew** — controlled: append SQL fragment and bind value in the
   same method call; `List` is insertion-ordered (`Rc<Vec>`), identical on all legs.
3. **Identifier-quote dialect drift** — controlled: pin ANSI `"` + `""`-doubling literally in Phorj;
   no driver call. Backtick/`[ ]` dialects are a v2 enum, still pure.
4. **Heterogeneous bind list typing** — the real *language* gap, not a determinism risk: `List` has no
   `Any`/union element today for `[18, true, "x"]`. Same wall that historically deferred `core.json`;
   already solved by the injected-enum trick → inject a `Bind` sum type (or wait for `Any`).
5. **Float binds rendering** — N/A on the build path: binds are *carried as values in the `List`*, never
   formatted into the SQL string, so the Ryū/14-digit float divergence never touches `Core.Sql`. (Only a
   downstream Tier-B `Core.Db` that stringifies a bind would meet it — out of scope.)
6. **mbstring absence** — N/A: SQL keywords and identifiers are ASCII; `str_replace`/`implode` are
   byte-level core fns.
7. **LIMIT/OFFSET as bound vs inline** — pin **inline integers** for LIMIT/OFFSET (they are `int`,
   trivially safe to render, and many drivers reject bound LIMIT); rendering an `int` is deterministic
   (no float, no locale).

---

## 9. Effort & recommendation

- **Effort: small–medium.** If `clone-with` injected classes carry the whole builder: ~1 injected
  prelude (`SQL_PRELUDE` const + `inject_sql_prelude`, copy of `inject_json_prelude`), the `Bind` sum
  type (or `Any` dependency), one `examples/sql/builder.phg` guide + README entry, differential gates it
  automatically. Closer to **small** if `Any`/`Bind` is already available; **medium** because the
  heterogeneous-bind-list ergonomics need a design call (inject `Bind` now vs wait for `Any`).
- **Recommendation: ADOPT — but sequence after (or alongside) a heterogeneous-list story** (`Bind`
  injected enum is the unblock and is cheap). The builder itself is the lowest-risk Tier-A module in the
  whole batch: no natives strictly required, no Op, no PHP-specific emission, byte-identity free.
- **Feasibility: ~92%** (high). The 8% is entirely the `List`-of-mixed-binds ergonomics decision, not
  any byte-identity or std-only doubt.

## 10. Pairing note
`Core.Sql` (Tier A, this spike) is the deterministic front half; `Core.Db` (Tier B, `pure:false`, M6
`Transport`-quarantined, fixture-tested outside `differential.rs`) is the deferred execution half.
Ship `Core.Sql` standalone now — it is independently useful (generate SQL text, log it, hand to any
driver) and gateable; `Core.Db` follows the `Process`/`Env` quarantine precedent later.
